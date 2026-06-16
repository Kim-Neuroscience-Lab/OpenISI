//! Enforced `.oisi` ↔ `docs/oisi.schema.json` contract.
//!
//! `oisi.schema.json` is a hand-maintained catalog of every group, dataset, and
//! attribute an `.oisi` file may contain. Hand-maintained docs drift from code
//! silently — that is exactly how `/analysis_state` was once written by the code
//! but missing from the schema. This test makes that drift a CI failure instead
//! of a latent lie.
//!
//! Direction checked here: **no undocumented entity** — every group, dataset,
//! and attribute physically present in a real analyzed `.oisi` must be named in
//! the schema. That is the direction that catches "code grew a field, nobody
//! updated the doc". (The reverse — every documented item is present — needs a
//! fixture that exercises every `present_when` branch, including the raw
//! `/acquisition` tree this analyzed fixture lacks; the acquisition-write side
//! is contract-checked next to `write_oisi` in the `openisi` crate.)
//!
//! The schema is a *descriptive* catalog with an irregular shape (its named
//! children live under `properties` / `datasets` / `subgroups` / `subdatasets` /
//! `attributes`), so the documented-name set is collected by recursively
//! gathering the keys of those containers. Over-collecting schema-meta keys
//! (`dtype`, `shape`, …) only ever *over-permits*, so it cannot cause a false
//! drift failure — only the present⊆documented direction is asserted.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

/// Containers in the schema whose object keys name an HDF5 entity.
const NAME_CONTAINERS: &[&str] = &[
    "properties",
    "datasets",
    "subgroups",
    "subdatasets",
    "attributes",
];

/// Recursively collect every entity name the schema documents (basename only;
/// `/complex_maps` → `complex_maps`, `/acquisition/camera` → `camera`).
fn documented_names(schema: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    fn walk(v: &Value, names: &mut BTreeSet<String>) {
        match v {
            Value::Object(map) => {
                for (key, child) in map {
                    if NAME_CONTAINERS.contains(&key.as_str()) {
                        if let Value::Object(children) = child {
                            for name in children.keys() {
                                let base = name.rsplit('/').next().unwrap_or(name);
                                names.insert(base.to_string());
                            }
                        }
                    }
                    walk(child, names);
                }
            }
            Value::Array(arr) => arr.iter().for_each(|i| walk(i, names)),
            _ => {}
        }
    }
    walk(schema, &mut names);
    names
}

/// Walk a real `.oisi` group tree, recording every present entity NOT documented.
fn collect_undocumented(
    path: &str,
    group: &hdf5::Group,
    documented: &BTreeSet<String>,
    out: &mut Vec<String>,
) {
    // `/analysis_state` attributes are dynamic per-stage fingerprint keys
    // (e.g. `retinotopy`); the schema documents the *pattern*
    // (`<stage fingerprint_key>`), not each name, so don't check them.
    if path != "/analysis_state" {
        for a in group.attr_names().unwrap_or_default() {
            if !documented.contains(&a) {
                out.push(format!("attribute {path}@{a}"));
            }
        }
    }
    for name in group.member_names().unwrap_or_default() {
        let child = if path == "/" {
            format!("/{name}")
        } else {
            format!("{path}/{name}")
        };
        if let Ok(sub) = group.group(&name) {
            if !documented.contains(&name) {
                out.push(format!("group {child}"));
            }
            collect_undocumented(&child, &sub, documented, out);
        } else if let Ok(ds) = group.dataset(&name) {
            if !documented.contains(&name) {
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

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn analyzed_oisi_has_no_entity_undocumented_by_schema() {
    let schema_path = manifest_dir().join("../../docs/oisi.schema.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", schema_path.display())),
    )
    .expect("oisi.schema.json must be valid JSON");
    let documented = documented_names(&schema);
    // Sanity: the collector found a real catalog, not an empty set.
    assert!(
        documented.contains("complex_maps") && documented.contains("analysis_state"),
        "schema-name collector returned an implausible set: {documented:?}"
    );

    let oisi = manifest_dir().join("tests/fixtures/baseline/R43_smoke.baseline.oisi");
    let file = hdf5::File::open(&oisi)
        .unwrap_or_else(|e| panic!("open {}: {e}", oisi.display()));

    let mut undocumented = Vec::new();
    // Root attributes (on "/").
    for a in file.attr_names().unwrap_or_default() {
        if !documented.contains(&a) {
            undocumented.push(format!("root attribute @{a}"));
        }
    }
    collect_undocumented("/", &file, &documented, &mut undocumented);

    assert!(
        undocumented.is_empty(),
        "these entities exist in the .oisi but are undocumented in \
         docs/oisi.schema.json (schema drift — document them):\n  {}",
        undocumented.join("\n  ")
    );
}
