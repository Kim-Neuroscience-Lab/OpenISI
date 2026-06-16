//! Enforced `.oisi` ↔ schema contract, against the Rust single source of truth
//! ([`isi_analysis::oisi_schema::SCHEMA`]).
//!
//! Two guarantees are locked here:
//!
//! 1. **The doc is generated, not hand-maintained.** `docs/oisi.schema.json` must
//!    equal [`oisi_schema::to_json_schema`]. Regenerate after editing `SCHEMA`:
//!    `OISI_REGEN_SCHEMA=1 cargo test -p isi-analysis --test oisi_schema_contract`.
//! 2. **The schema matches reality.** A real analyzed `.oisi` is checked against
//!    `SCHEMA` in both directions: no present entity is undocumented (catches
//!    "code grew a field"), and every always-present documented entity in a
//!    present group exists (catches "schema documents a field the code dropped").
//!
//! The acquisition-write side (`/acquisition`, `/hardware`) is contract-checked
//! against the same `SCHEMA` next to `write_oisi` in the `openisi` crate, since
//! only that crate can produce a raw capture file.

use std::collections::BTreeSet;
use std::path::PathBuf;

use isi_analysis::oisi_schema::{self, Group, Presence, SCHEMA};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema_doc_path() -> PathBuf {
    manifest_dir().join("../../docs/oisi.schema.json")
}

/// Every entity name (basename) the schema documents.
fn documented_basenames() -> BTreeSet<String> {
    fn add_group(g: &Group, s: &mut BTreeSet<String>) {
        s.insert(g.path.rsplit('/').next().unwrap_or(g.path).to_string());
        for a in g.attrs {
            s.insert(a.name.to_string());
        }
        for d in g.datasets {
            s.insert(d.name.to_string());
            for a in d.attrs {
                s.insert(a.name.to_string());
            }
        }
        for sub in g.subgroups {
            add_group(sub, s);
        }
    }
    let mut s = BTreeSet::new();
    for a in SCHEMA.root_attrs {
        s.insert(a.name.to_string());
    }
    for d in SCHEMA.root_datasets {
        s.insert(d.name.to_string());
    }
    for g in SCHEMA.groups {
        add_group(g, &mut s);
    }
    s
}

/// Paths of groups whose attributes are dynamically named (e.g. `/analysis_state`).
fn dynamic_attr_paths() -> BTreeSet<String> {
    fn rec(g: &Group, s: &mut BTreeSet<String>) {
        if g.dynamic_attrs.is_some() {
            s.insert(g.path.to_string());
        }
        for sub in g.subgroups {
            rec(sub, s);
        }
    }
    let mut s = BTreeSet::new();
    for g in SCHEMA.groups {
        rec(g, &mut s);
    }
    s
}

// --- Direction A: nothing present is undocumented ---------------------------

fn collect_undocumented(
    path: &str,
    group: &hdf5::Group,
    documented: &BTreeSet<String>,
    dynamic: &BTreeSet<String>,
    out: &mut Vec<String>,
) {
    if !dynamic.contains(path) {
        for a in group.attr_names().unwrap_or_default() {
            if !documented.contains(&a) {
                out.push(format!("attribute {path}@{a}"));
            }
        }
    }
    for nm in group.member_names().unwrap_or_default() {
        let child = if path == "/" {
            format!("/{nm}")
        } else {
            format!("{path}/{nm}")
        };
        if let Ok(sub) = group.group(&nm) {
            if !documented.contains(&nm) {
                out.push(format!("group {child}"));
            }
            collect_undocumented(&child, &sub, documented, dynamic, out);
        } else if let Ok(ds) = group.dataset(&nm) {
            if !documented.contains(&nm) {
                out.push(format!("dataset {child}"));
            }
            for a in ds.attr_names().unwrap_or_default() {
                if !documented.contains(&a) {
                    out.push(format!("attribute {child}@{a}"));
                }
            }
        }
    }
}

// --- Direction B: every always-present documented entity exists -------------

fn is_always(p: Presence) -> bool {
    matches!(p, Presence::Always)
}

fn collect_missing(file: &hdf5::File, out: &mut Vec<String>) {
    let root_attrs: BTreeSet<String> = file.attr_names().unwrap_or_default().into_iter().collect();
    for a in SCHEMA.root_attrs {
        if is_always(a.presence) && !root_attrs.contains(a.name) {
            out.push(format!("root attribute @{}", a.name));
        }
    }
    for g in SCHEMA.groups {
        check_group_present(file, g, out);
    }
}

fn check_group_present(file: &hdf5::File, g: &Group, out: &mut Vec<String>) {
    let rel = g.path.trim_start_matches('/');
    let Ok(grp) = file.group(rel) else {
        return; // group absent → its own (conditional) presence is not checked here
    };
    let attrs: BTreeSet<String> = grp.attr_names().unwrap_or_default().into_iter().collect();
    for a in g.attrs {
        if is_always(a.presence) && !attrs.contains(a.name) {
            out.push(format!("attribute {}@{}", g.path, a.name));
        }
    }
    let members: BTreeSet<String> = grp.member_names().unwrap_or_default().into_iter().collect();
    for d in g.datasets {
        if is_always(d.presence) && !members.contains(d.name) {
            out.push(format!("dataset {}/{}", g.path, d.name));
        }
    }
    for sub in g.subgroups {
        let present = file.group(sub.path.trim_start_matches('/')).is_ok();
        if is_always(sub.presence) && !present {
            out.push(format!("subgroup {}", sub.path));
        }
        if present {
            check_group_present(file, sub, out);
        }
    }
}

/// Shared entry point reused by the acquisition-side contract test in `openisi`.
pub fn assert_oisi_matches_schema(oisi: &std::path::Path) {
    let file = hdf5::File::open(oisi).unwrap_or_else(|e| panic!("open {}: {e}", oisi.display()));
    let documented = documented_basenames();
    let dynamic = dynamic_attr_paths();

    let mut undocumented = Vec::new();
    for a in file.attr_names().unwrap_or_default() {
        if !documented.contains(&a) {
            undocumented.push(format!("root attribute @{a}"));
        }
    }
    collect_undocumented("/", &file, &documented, &dynamic, &mut undocumented);
    assert!(
        undocumented.is_empty(),
        "{}: entities present but UNDOCUMENTED in oisi_schema::SCHEMA (drift):\n  {}",
        oisi.display(),
        undocumented.join("\n  ")
    );

    let mut missing = Vec::new();
    collect_missing(&file, &mut missing);
    assert!(
        missing.is_empty(),
        "{}: entities SCHEMA marks always-present but MISSING from the file:\n  {}",
        oisi.display(),
        missing.join("\n  ")
    );
}

#[test]
fn committed_schema_doc_matches_generated() {
    let generated = oisi_schema::to_json_schema();
    let path = schema_doc_path();
    if std::env::var("OISI_REGEN_SCHEMA").is_ok() {
        let pretty = serde_json::to_string_pretty(&generated).expect("serialize schema") + "\n";
        std::fs::write(&path, pretty).expect("write schema doc");
        return;
    }
    let committed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    )
    .expect("committed oisi.schema.json must be valid JSON");
    assert_eq!(
        committed, generated,
        "docs/oisi.schema.json is stale vs oisi_schema::SCHEMA — regenerate with \
         `OISI_REGEN_SCHEMA=1 cargo test -p isi-analysis --test oisi_schema_contract`"
    );
}

#[test]
fn analyzed_oisi_matches_schema() {
    // Sanity: the SCHEMA flatteners found a real catalog.
    let documented = documented_basenames();
    assert!(documented.contains("complex_maps") && documented.contains("analysis_state"));

    assert_oisi_matches_schema(
        &manifest_dir().join("tests/fixtures/baseline/R43_smoke.baseline.oisi"),
    );
}
