#!/usr/bin/env python
"""End-to-end validation gate for the OpenISI -> NWB export.

Builds a synthetic *full*-surface `.oisi` (a real analysis-output fixture augmented
with a realistic `/acquisition` group), exports it to NWB, and asserts:

  1. ``nwbinspector`` finds **no** issues (the reference best-practices validator);
  2. the round-trip is lossless (every map / complex map / segmentation area /
     anatomical image / raw frame / sweep / provenance field byte-identical).

Exits 0 only if both hold. This is the Phase-4 conformance gate — run it in CI
(see ``tools/export_nwb/README.md``). Realistic dimensions (T >> H,W; jittered
timestamps) so the export is judged on its merits, not on synthetic-fixture
heuristic artifacts.

Usage: python validate_export.py [--fixture path.oisi] [--keep]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import warnings

import h5py
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
NS = os.path.join(ROOT, "ndx-openisi", "spec", "ndx-openisi.namespace.yaml")
META = os.path.join(HERE, "metadata.example.json")
DEFAULT_FIXTURE = os.path.join(
    ROOT, "crates", "isi-analysis", "tests", "fixtures", "baseline", "R43_smoke.baseline.oisi"
)

sys.path.insert(0, HERE)
import export_oisi_to_nwb as exporter  # noqa: E402
import roundtrip_check  # noqa: E402


def augment_with_acquisition(path: str) -> None:
    """Add a realistic `/acquisition` group (raw frames + multi-clock timing +
    sweep schedule), so the export's acquisition branch is exercised. Faithful to
    the documented `.oisi` layout; dimensions chosen so nwbinspector heuristics
    (time-axis orientation, regular-timestamp) judge the export, not the fixture."""
    rng = np.random.RandomState(0)
    with h5py.File(path, "r") as f:
        H, W = f["results/vfs"].shape
    T, S = 300, 600
    # Jittered camera timeline (real cameras are not perfectly periodic).
    cam_dt = 0.0334 + rng.normal(0, 0.0008, T)
    cam_sec = np.cumsum(cam_dt) - cam_dt[0]
    cam_hw_us = (cam_sec * 1e6).astype("i8")
    cam_sys_us = cam_hw_us + 50 + rng.randint(-5, 6, T)
    with h5py.File(path, "a") as f:
        cam = f.create_group("acquisition/camera")
        cam.create_dataset("frames", data=(rng.rand(T, H, W) * 1000).astype("u2"),
                           chunks=(1, H, W), compression="gzip", compression_opts=4)
        cam.create_dataset("timestamps_sec", data=cam_sec.astype("f8"))
        cam.create_dataset("hardware_timestamps_us", data=cam_hw_us)
        cam.create_dataset("system_timestamps_us", data=cam_sys_us)
        cam.create_dataset("sequence_numbers", data=np.arange(T, dtype="i8"))
        stim = f.create_group("acquisition/stimulus")
        stim_us = np.cumsum(16670 + rng.randint(-200, 200, S)).astype("i8")
        stim.create_dataset("timestamps_us", data=stim_us)
        stim.create_dataset("timestamps_sec", data=(stim_us / 1e6).astype("f8"))
        stim.create_dataset("frame_deltas_us", data=np.diff(stim_us))
        stim.create_dataset("dropped_frame_indices", data=np.array([], dtype="i8"))
        seq = ["LR", "RL", "TB", "BT"]
        sch = f.create_group("acquisition/schedule")
        sch.attrs["sweep_sequence"] = json.dumps(seq)
        starts = np.array([0.0, 2.5, 5.0, 7.5])
        stops = starts + 2.4
        sch.create_dataset("sweep_start_sec", data=starts)
        sch.create_dataset("sweep_end_sec", data=stops)
        sch.create_dataset("sweep_start_us", data=(starts * 1e6).astype("i8"))
        sch.create_dataset("sweep_end_us", data=(stops * 1e6).astype("i8"))
        cs = f.create_group("acquisition/clock_sync")
        cs.attrs["t0_system_us"] = float(cam_sys_us[0])
        cs.attrs["start_offset_us"] = 50.0
        cs.attrs["end_offset_us"] = 52.0
        cs.attrs["drift_us"] = 2.0
        q = f.create_group("acquisition/quality")
        q.create_dataset("camera_frame_deltas_us", data=np.diff(cam_hw_us))
        q.create_dataset("camera_sequence_gaps", data=np.array([], dtype="u4"))
        q.create_dataset("mean_frame_intensity", data=(rng.rand(T) * 500).astype("f4"))
        q.attrs["camera_drops_total"] = np.uint32(0)
        q.attrs["stimulus_drops_total"] = np.uint32(0)
        q.attrs["stimulus_timing_validatable"] = np.uint32(1)
        q.attrs["display_scanout"] = "physical"
        q.attrs["acquisition_complete"] = np.uint8(1)


def _dandi_validate(nwb_path: str, tmpdir: str):
    """Run DANDI's own validator on the export — the metadata conformance the
    archive enforces. Needs NO account (validation only; upload is out of scope).
    Returns (status, detail): status is True (clean) / False (errors) / None
    (dandi not installed → skipped)."""
    try:
        import dandi  # noqa: F401
    except ImportError:
        return None, "dandi not installed (skipped)"
    import subprocess
    ddir = os.path.join(tmpdir, "dandiset")
    os.makedirs(ddir, exist_ok=True)
    shutil.copy(nwb_path, ddir)
    with open(os.path.join(ddir, "dandiset.yaml"), "w", encoding="utf-8") as fh:
        fh.write("name: OpenISI export conformance\n"
                 "description: local DANDI metadata conformance check\n"
                 "identifier: 'DANDI:000000'\n")
    dandi_exe = shutil.which("dandi") or "dandi"
    # Run from inside the dandiset dir (organize/validate resolve the dandiset
    # from the cwd + dandiset.yaml), matching the documented CLI usage.
    org = subprocess.run([dandi_exe, "organize", ".", "-f", "move"],
                         cwd=ddir, capture_output=True, text=True)
    if org.returncode != 0:
        return False, f"dandi organize failed: {(org.stdout + org.stderr).strip()[:300]}"
    val = subprocess.run([dandi_exe, "validate", "."], cwd=ddir, capture_output=True, text=True)
    out = (val.stdout + val.stderr)
    clean = val.returncode == 0 and "No errors found" in out
    return clean, ("No errors found" if clean else out.strip()[:400])


def validate(fixture: str, keep: bool) -> int:
    tmpdir = tempfile.mkdtemp(prefix="oisi_nwb_")
    oisi = os.path.join(tmpdir, "full.oisi")
    nwb = os.path.join(tmpdir, "full.nwb")
    shutil.copy(fixture, oisi)
    augment_with_acquisition(oisi)

    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        with open(META, encoding="utf-8") as fh:
            meta = json.load(fh)
        exporter.convert(oisi, nwb, NS, meta)

        import nwbinspector as ni
        msgs = list(ni.inspect_nwbfile(nwbfile_path=nwb))

    print("\n=== nwbinspector ===")
    if msgs:
        for m in msgs:
            print(f"  [{m.importance.name}] {m.check_function_name}: {m.message}")
    else:
        print("  No issues found!")

    print("\n=== round-trip fidelity ===")
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        rt = roundtrip_check.run(oisi, nwb, NS)

    print("\n=== DANDI metadata conformance (dandi validate) ===")
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        dandi_ok, dandi_detail = _dandi_validate(nwb, tmpdir)
    print(f"  {dandi_detail}")

    # Pass requires nwbinspector clean + lossless round-trip + (dandi clean OR
    # dandi absent). A present-but-failing dandi is a hard fail.
    ok = (not msgs) and (rt == 0) and (dandi_ok is not False)
    if keep:
        print(f"\nartifacts kept in {tmpdir}")
    else:
        shutil.rmtree(tmpdir, ignore_errors=True)
    dandi_word = {True: "clean", False: "ERRORS", None: "skipped"}[dandi_ok]
    print(f"\n{'PASS' if ok else 'FAIL'}: nwbinspector {'clean' if not msgs else f'{len(msgs)} issue(s)'}, "
          f"round-trip {'lossless' if rt == 0 else 'mismatch'}, dandi {dandi_word}")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixture", default=DEFAULT_FIXTURE)
    ap.add_argument("--keep", action="store_true", help="keep the generated .oisi/.nwb")
    args = ap.parse_args()
    sys.exit(validate(args.fixture, args.keep))


if __name__ == "__main__":
    main()
