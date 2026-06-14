#!/usr/bin/env python
"""Round-trip fidelity check: read an exported `.nwb` back through pynwb and assert
its retinotopy maps / complex maps / segmentation / anatomical / provenance match
the source `.oisi` byte-for-byte. This is the export's correctness gate — it proves
the conversion is lossless and that the NWB file is readable by the reference
implementation (not just structurally valid).

Usage: python roundtrip_check.py SOURCE.oisi EXPORT.nwb [--namespace ...]
Exit 0 = all checks pass; non-zero on any mismatch.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import warnings

import h5py
import numpy as np
from pynwb import NWBHDF5IO, load_namespaces

_SEG_ONLY = {"area_labels", "area_signs"}
_FAILS: list[str] = []


def _check(cond: bool, msg: str) -> None:
    if cond:
        print(f"  ok   {msg}")
    else:
        print(f"  FAIL {msg}")
        _FAILS.append(msg)


def run(oisi_path: str, nwb_path: str, namespace_path: str) -> int:
    load_namespaces(namespace_path)
    with h5py.File(oisi_path, "r") as f, NWBHDF5IO(nwb_path, "r") as io:
        nwb = io.read()

        # ── Retinotopy maps (ndx RetinotopyMaps) — only when the source was
        #    analyzed (a raw, pre-analysis .oisi correctly has neither). ──
        rmaps = (nwb.processing["ophys"].data_interfaces.get("retinotopy")
                 if "ophys" in nwb.processing else None)
        if "results" in f:
            _check(rmaps is not None, "retinotopy container present")
        if rmaps is not None and "results" in f:
            # retinotopy_maps / complex_maps are LabelledDict {name: obj}.
            nwb_maps = dict(rmaps.retinotopy_maps)
            for name in sorted(f["results"].keys()):
                ds = f["results"][name]
                if name in _SEG_ONLY or not isinstance(ds, h5py.Dataset) or ds.ndim != 2:
                    continue
                got = nwb_maps.get(name)
                if got is None:
                    _check(False, f"result map '{name}' present in NWB")
                    continue
                _check(np.array_equal(np.asarray(got.data), ds[()], equal_nan=ds.dtype.kind == "f"),
                       f"result map '{name}' values byte-identical")
                # MapMeta render contract preserved
                _check(str(got.palette) == str(ds.attrs.get("palette", b"").decode()
                       if isinstance(ds.attrs.get("palette"), bytes) else ds.attrs.get("palette", "")),
                       f"result map '{name}' palette preserved")

            # ── Complex maps ──
            nwb_cm = dict(rmaps.complex_maps) if rmaps.complex_maps else {}
            if "complex_maps" in f:
                for name in sorted(f["complex_maps"].keys()):
                    ds = f["complex_maps"][name]
                    got = nwb_cm.get(name)
                    if got is None:
                        _check(False, f"complex map '{name}' present")
                        continue
                    _check(np.array_equal(np.asarray(got.data), ds[()], equal_nan=True),
                           f"complex map '{name}' values byte-identical")

            # ── Analysis provenance ──
            ap_src = f.attrs.get("analysis_params")
            if ap_src is not None:
                ap_src = ap_src.decode() if isinstance(ap_src, bytes) else str(ap_src)
                _check(rmaps.analysis_params == ap_src, "analysis_params provenance preserved")

        # ── Segmentation (PlaneSegmentation areas + signs) ──
        if "results" in f and "area_labels" in f["results"]:
            labels = f["results"]["area_labels"][()]
            signs = f["results"]["area_signs"][()] if "area_signs" in f["results"] else None
            ps = nwb.processing["ophys"]["image_segmentation"]["visual_areas"]
            uniq = [int(v) for v in np.unique(labels) if int(v) != 0]
            _check(len(ps.id) == len(uniq), f"segmentation ROI count == area count ({len(uniq)})")
            masks = ps["image_mask"][:]
            ok_masks = all(np.array_equal(masks[i], labels == k) for i, k in enumerate(uniq))
            _check(ok_masks, "every area ROI image_mask matches its label region")
            if signs is not None:
                got_signs = list(ps["sign"][:])
                exp_signs = [int(signs[k - 1]) for k in uniq]
                _check(got_signs == exp_signs, "per-area sign column matches area_signs")

        # ── Anatomical ──
        if "anatomical" in f and isinstance(f["anatomical"], h5py.Dataset):
            imgs = nwb.acquisition.get("anatomical_images")
            _check(imgs is not None and "anatomical" in imgs.images, "anatomical image present")
            if imgs is not None and "anatomical" in imgs.images:
                _check(np.array_equal(np.asarray(imgs.images["anatomical"].data), f["anatomical"][()]),
                       "anatomical image values byte-identical")

        # ── Sweep schedule (only when non-empty; a real acquisition can write an
        #    empty schedule, which the export correctly omits) ──
        if "acquisition/schedule/sweep_start_sec" in f and len(f["acquisition/schedule/sweep_start_sec"]) > 0:
            sw = nwb.intervals.get("sweeps") if nwb.intervals else None
            _check(sw is not None, "sweeps TimeIntervals present")
            if sw is not None:
                _check(np.allclose(np.asarray(sw["start_time"][:]),
                                   f["acquisition/schedule/sweep_start_sec"][()]),
                       "sweep start_times match")

        # ── Raw frames (if present) ──
        if "acquisition/camera/frames" in f:
            ops = nwb.acquisition.get("raw_frames")
            _check(ops is not None, "OnePhotonSeries raw_frames present")
            if ops is not None:
                _check(np.array_equal(np.asarray(ops.data), f["acquisition/camera/frames"][()]),
                       "raw frames byte-identical")

        # ── Subject present (DANDI requirement) ──
        _check(nwb.subject is not None and nwb.subject.species == "Mus musculus",
               "subject present with species")

    print(f"\n{'PASS' if not _FAILS else 'FAIL'}: {len(_FAILS)} mismatch(es)")
    return 1 if _FAILS else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("oisi")
    ap.add_argument("nwb")
    ap.add_argument("--namespace",
                    default=os.path.join(os.path.dirname(__file__), "..", "..", "ndx-openisi",
                                         "spec", "ndx-openisi.namespace.yaml"))
    args = ap.parse_args()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        sys.exit(run(args.oisi, args.nwb, os.path.abspath(args.namespace)))


if __name__ == "__main__":
    main()
