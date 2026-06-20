//! Enforced `.oisi` ↔ schema contract, against the Rust single source of truth
//! ([`isi_analysis::oisi_schema::SCHEMA`]).
//!
//! Two guarantees are locked here:
//!
//! 1. **The doc is generated, not hand-maintained.** `docs/oisi.schema.json` must
//!    equal [`oisi_schema::to_json_schema`]. Regenerate after editing `SCHEMA`:
//!    `OISI_REGEN_SCHEMA=1 cargo test -p isi-analysis --test oisi_schema_contract`.
//! 2. **The schema matches reality.** A real analyzed `.oisi` conforms to `SCHEMA`
//!    in both directions ([`oisi_schema::contract_violations`]): nothing present
//!    is undocumented, and every always-present documented entity exists.
//!
//! The acquisition-write side (`/acquisition`, `/hardware`) is checked against the
//! same `SCHEMA` and the same `contract_violations` next to `write_oisi` in the
//! `openisi` crate, since only that crate can produce a raw capture file.

use std::path::PathBuf;

use isi_analysis::oisi_schema;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn committed_schema_doc_matches_generated() {
    let generated = oisi_schema::to_json_schema();
    let path = manifest_dir().join("../../docs/oisi.schema.json");
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
fn analyzed_oisi_conforms_to_schema() {
    let oisi = manifest_dir().join("tests/fixtures/baseline/R43_smoke.baseline.oisi");
    // Dev-time validation against a real analyzed `.oisi` (gitignored real data,
    // never published). Absent on a clean checkout / general CI → SKIP loudly,
    // don't panic. The schema⇄docs golden check above runs everywhere; this
    // real-file conformance check runs where the data lives.
    if !oisi.exists() {
        eprintln!(
            "SKIP analyzed_oisi_conforms_to_schema: baseline absent (gitignored real data): {}",
            oisi.display()
        );
        return;
    }
    let file = hdf5::File::open(&oisi).unwrap_or_else(|e| panic!("open {}: {e}", oisi.display()));
    let violations = oisi_schema::contract_violations(&file);
    assert!(
        violations.is_empty(),
        "{} does not conform to oisi_schema::SCHEMA:\n  {}",
        oisi.display(),
        violations.join("\n  ")
    );
}
