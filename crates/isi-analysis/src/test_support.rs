//! Shared helpers for the in-crate golden tests. Golden fixtures are raw
//! little-endian binary blobs (`*.bin`); this is the ONE place that decodes
//! them and the one comparison helper, so the byte layout and the
//! disagreement-count logic aren't re-derived in every `mod golden`.
//!
//! Gated `#[cfg(test)]` at the `mod` declaration in `lib.rs`, so none of this
//! ships in a release build.

use ndarray::Array2;

/// Decode a little-endian `f64` blob (row-major) into a flat `Vec`.
pub(crate) fn load_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Decode a little-endian `f32` blob (row-major) into a flat `Vec`.
pub(crate) fn load_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Decode a little-endian `i32` blob (row-major) into a flat `Vec`.
pub(crate) fn load_i32(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Count pixels where a bool result array disagrees with a `u8` golden mask
/// (row-major, `0`/`1`). Uses the array's own dims — no stride argument.
pub(crate) fn count_differing(ours: &Array2<bool>, golden: &[u8]) -> usize {
    let (h, w) = ours.dim();
    let mut d = 0usize;
    for r in 0..h {
        for c in 0..w {
            if (ours[[r, c]] as u8) != golden[r * w + c] {
                d += 1;
            }
        }
    }
    d
}

/// Live bridge to the GENUINE reference oracles, each running in its own
/// `uv`-locked, period-correct environment under `tests/oracle/<name>/`. The
/// oracle is the field's actual code, EXECUTED on every run — never a
/// transcription, never a frozen fixture. This module only marshals f64 arrays
/// across the process boundary; the reference computes the result.
///
/// Gated behind the `oracle_live` feature so the default test suite needs no
/// interpreter. Requires `uv` (on PATH or via `OPENISI_UV`); CI runs the oracle
/// suite with `--features oracle_live`.
#[cfg(feature = "oracle_live")]
pub(crate) mod oracle {
    use ndarray::Array2;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn uv() -> String {
        std::env::var("OPENISI_UV").unwrap_or_else(|_| "uv".to_string())
    }

    /// One typed array returned by a genuine oracle call — the reference's raw
    /// bytes + numpy dtype + shape, decoded on demand by the test.
    pub(crate) struct OracleOut {
        pub dtype: String,
        pub shape: (usize, usize),
        pub bytes: Vec<u8>,
    }

    impl OracleOut {
        /// Decode as f64 (numpy `<f8`).
        pub(crate) fn f64(&self) -> Array2<f64> {
            assert_eq!(self.dtype, "<f8", "expected f64 output, got {}", self.dtype);
            let data: Vec<f64> = self
                .bytes
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                .collect();
            Array2::from_shape_vec(self.shape, data).expect("shape f64 output")
        }

        /// Decode an integer label/marker map as i32 (numpy `<i4`/`<i8`).
        pub(crate) fn i32(&self) -> Array2<i32> {
            let data: Vec<i32> = match self.dtype.as_str() {
                "<i4" | "<u4" => self
                    .bytes
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
                "<i8" | "<u8" => self
                    .bytes
                    .chunks_exact(8)
                    .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as i32)
                    .collect(),
                other => panic!("i32() unsupported dtype {other}"),
            };
            Array2::from_shape_vec(self.shape, data).expect("shape i32 output")
        }

        /// Decode as a boolean mask (any nonzero element → true). Accepts the
        /// reference's integer/bool/float dtypes for binary maps (a reference that
        /// builds a 0/1 map in a float array — e.g. `getVisualSpace` — is common).
        pub(crate) fn bool(&self) -> Array2<bool> {
            let data: Vec<bool> = match self.dtype.as_str() {
                // Float 0/1 maps: nonzero (and non-NaN) → true.
                "<f8" => self
                    .bytes
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()) != 0.0)
                    .collect(),
                "<f4" => self
                    .bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()) != 0.0)
                    .collect(),
                _ => {
                    let elem = match self.dtype.as_str() {
                        "|b1" | "|u1" | "|i1" => 1,
                        "<i2" | "<u2" => 2,
                        "<i4" | "<u4" => 4,
                        "<i8" | "<u8" => 8,
                        other => panic!("bool() unsupported dtype {other}"),
                    };
                    self.bytes
                        .chunks_exact(elem)
                        .map(|c| c.iter().any(|&b| b != 0))
                        .collect()
                }
            };
            Array2::from_shape_vec(self.shape, data).expect("shape bool output")
        }
    }

    /// Call a genuine function in the **NeuroAnalysisTools** oracle env returning
    /// typed outputs (any dtype the reference produces). The f64 convenience
    /// wrapper [`nat`] is built on this.
    pub(crate) fn nat_raw(
        func: &str,
        inputs: &[Array2<f64>],
        params: &[(&str, f64)],
    ) -> Vec<OracleOut> {
        let nat_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/nat");
        let bridge = nat_dir.join("bridge.py");
        let work = std::env::temp_dir().join(format!(
            "openisi_oracle_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&work).expect("oracle workdir");

        // Marshal inputs as raw little-endian f64 + a JSON request.
        let input_specs: Vec<serde_json::Value> = inputs
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let (h, w) = a.dim();
                let p = work.join(format!("in{i}.bin"));
                let mut f = std::fs::File::create(&p).expect("write input");
                for &v in a.iter() {
                    f.write_all(&v.to_le_bytes()).expect("write input bytes");
                }
                serde_json::json!({ "path": p.to_string_lossy(), "dtype": "<f8", "shape": [h, w] })
            })
            .collect();
        let params_obj: serde_json::Map<String, serde_json::Value> = params
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
            .collect();
        let req = serde_json::json!({
            "fn": func,
            "inputs": input_specs,
            "params": serde_json::Value::Object(params_obj),
            "out_dir": work.to_string_lossy(),
        });
        let req_path = work.join("req.json");
        std::fs::write(&req_path, serde_json::to_vec(&req).unwrap()).expect("write request");

        // Run the genuine reference in its OWN locked env (uv provisions it).
        let out = Command::new(uv())
            .args(["run", "--project"])
            .arg(&nat_dir)
            .arg("python")
            .arg(&bridge)
            .arg(&req_path)
            .output()
            .expect("spawn uv — put `uv` on PATH or set OPENISI_UV");
        assert!(
            out.status.success(),
            "NAT oracle bridge failed for {func:?}:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );

        let resp: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("parse bridge stdout JSON");
        resp["outputs"]
            .as_array()
            .expect("outputs array")
            .iter()
            .map(|o| {
                let file = o["file"].as_str().expect("output file");
                let shape = o["shape"].as_array().expect("output shape");
                let h = shape[0].as_u64().unwrap() as usize;
                let w = shape[1].as_u64().unwrap() as usize;
                OracleOut {
                    dtype: o["dtype"].as_str().expect("output dtype").to_string(),
                    shape: (h, w),
                    bytes: std::fs::read(file).expect("read output"),
                }
            })
            .collect()
    }

    /// f64 convenience wrapper over [`nat_raw`] (most NAT outputs are f64).
    pub(crate) fn nat(func: &str, inputs: &[Array2<f64>], params: &[(&str, f64)]) -> Vec<Array2<f64>> {
        nat_raw(func, inputs, params).iter().map(OracleOut::f64).collect()
    }

    fn octave() -> String {
        std::env::var("OPENISI_OCTAVE").unwrap_or_else(|_| "octave-cli".to_string())
    }

    /// Call a genuine function in the **SNLC / Garrett** oracle env (`tests/oracle/
    /// snlc/`), executed via Octave against the real `reference/ISI/*.m`. Returns
    /// the reference's f64 outputs. (Octave≈MATLAB is the flagged irreducible gap.)
    pub(crate) fn snlc(func: &str, inputs: &[Array2<f64>], params: &[(&str, f64)]) -> Vec<Array2<f64>> {
        let snlc_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/snlc");
        let bridge = snlc_dir.join("bridge.m");
        let work = std::env::temp_dir().join(format!(
            "openisi_oracle_snlc_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&work).expect("oracle workdir");

        let input_specs: Vec<serde_json::Value> = inputs
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let (h, w) = a.dim();
                let p = work.join(format!("in{i}.bin"));
                let mut f = std::fs::File::create(&p).expect("write input");
                for &v in a.iter() {
                    f.write_all(&v.to_le_bytes()).expect("write input bytes");
                }
                serde_json::json!({ "path": p.to_string_lossy(), "dtype": "<f8", "shape": [h, w] })
            })
            .collect();
        let params_obj: serde_json::Map<String, serde_json::Value> = params
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
            .collect();
        let req = serde_json::json!({
            "fn": func,
            "inputs": input_specs,
            "params": serde_json::Value::Object(params_obj),
            "out_dir": work.to_string_lossy(),
        });
        let req_path = work.join("req.json");
        std::fs::write(&req_path, serde_json::to_vec(&req).unwrap()).expect("write request");

        let out = Command::new(octave())
            .args(["--norc", "-q"])
            .arg(&bridge)
            .arg(&req_path)
            .output()
            .expect("spawn octave-cli — put it on PATH or set OPENISI_OCTAVE");
        assert!(
            out.status.success(),
            "SNLC oracle bridge failed for {func:?}:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );

        let resp: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("parse bridge stdout JSON");
        // Octave's jsonencode emits a single struct as an object, an array of structs
        // as an array — normalise to a slice.
        let outs_val = &resp["outputs"];
        let outs: Vec<&serde_json::Value> = match outs_val {
            serde_json::Value::Array(a) => a.iter().collect(),
            obj => vec![obj],
        };
        outs.iter()
            .map(|o| {
                let file = o["file"].as_str().expect("output file");
                let shape = o["shape"].as_array().expect("output shape");
                let h = shape[0].as_u64().unwrap() as usize;
                let w = shape[1].as_u64().unwrap() as usize;
                let bytes = std::fs::read(file).expect("read output");
                let data: Vec<f64> = bytes
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect();
                Array2::from_shape_vec((h, w), data).expect("shape output")
            })
            .collect()
    }
}
