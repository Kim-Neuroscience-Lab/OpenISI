#!/usr/bin/env python3
"""PRISTINE bridge to the GENUINE NeuroAnalysisTools 3.1.0 oracle, run inside this
oracle's own locked, period-correct uv environment (see pyproject.toml).

NO shim. In a numpy<1.24 environment the reference's `np.int` works natively and its
`xrange` uses are dead code — so the vendored reference runs AS-IS, byte-pristine.
This file contains NO oracle algorithm: only (a) array marshalling across the process
boundary and (b) a dispatch table mapping a function id to a DIRECT call of the
genuine reference. No per-golden scripts, no frozen fixtures — the oracle is computed
live on every run.

Protocol:
  argv[1] = request JSON:
    {"fn": str,
     "inputs": [{"path": str, "dtype": str (numpy '<f8' etc), "shape": [int,...]}],
     "params": {..scalars..},
     "out_dir": str}
  stdout  = response JSON: {"outputs": [{"file": str, "dtype": str, "shape": [int]}]}
"""
import json
import os
import sys

import numpy as np

_HERE = os.path.dirname(os.path.abspath(__file__))
# crates/isi-analysis/tests/oracle/nat/ -> repo root is five levels up.
_REPO = os.path.abspath(os.path.join(_HERE, "..", "..", "..", "..", ".."))
sys.path.insert(0, os.path.join(_REPO, "reference", "NeuroAnalysisTools"))

import NeuroAnalysisTools.RetinotopicMapping as RM  # noqa: E402  genuine, pristine
import NeuroAnalysisTools.core.ImageAnalysis as IA  # noqa: E402  genuine, pristine

VERSION = "NeuroAnalysisTools 3.1.0 (locked env: py3.10, numpy 1.23.5, scipy 1.9.3, scikit-image 0.19.3)"


def _load(spec):
    return np.fromfile(spec["path"], dtype=spec["dtype"]).reshape(spec["shape"])


def dispatch(fn, x, p):
    """Map a function id to a DIRECT call of the genuine reference. Each arm unpacks
    args and calls the real function — nothing else. (No oracle logic lives here.)"""
    if fn == "visualSignMap":
        return [RM.visualSignMap(x[0], x[1])]
    if fn == "dilationPatches2":
        return [RM.dilationPatches2(x[0], dilationIter=int(p["dilationIter"]),
                                    borderWidth=int(p["borderWidth"]))]
    if fn == "mergePatches":
        # Genuine mergePatches raises LookupError when the two patches are too far
        # apart to merge (>1 CC). Report that decision as a 1x1 flag alongside the
        # closing result, so the Rust side compares both.
        try:
            spc = RM.mergePatches(x[0], x[1], borderWidth=int(p["borderWidth"]))
            return [np.asarray(spc), np.array([[1]], dtype=np.int8)]
        except LookupError:
            return [np.zeros_like(x[0], dtype=np.int8), np.array([[0]], dtype=np.int8)]
    if fn == "localMin":
        return [RM.localMin(x[0], p["binSize"])]
    if fn == "eccentricityMap":
        return [RM.eccentricityMap(x[0], x[1], p["altCenter"], p["aziCenter"])]
    if fn == "is_adjacent":
        return [np.array([[IA.is_adjacent(x[0], x[1], borderWidth=int(p["borderWidth"]))]],
                         dtype=np.int8)]
    # --- library-primitive oracles: the LIBRARY (pinned in this env: scipy 1.9.3,
    # numpy 1.23.5) is the genuine oracle. Pure single-call pass-throughs, computed
    # live so no frozen fixture can drift from the library (condition 6). ---
    if fn == "scipy_gaussian_filter":
        import scipy.ndimage as _sni
        return [_sni.gaussian_filter(x[0], p["sigma"], mode="reflect", truncate=4.0)]
    if fn == "scipy_label":
        import scipy.ndimage as _sni
        labels, _n = _sni.label(x[0] != 0)  # default structure = 4-conn cross
        return [labels.astype(np.int32)]
    if fn == "skimage_skeletonize":
        import skimage.morphology as _sm
        return [_sm.skeletonize(x[0] != 0).astype(np.int8)]
    if fn == "numpy_fft_bin":
        # Genuine oracle = numpy's FFT. x[0] is a 3-D movie [n,H,W]; return the
        # real and imaginary parts of a single temporal bin (our single-frequency
        # DFT kernel exp(-2pi i freq dt t) at freq*dt = 1/n equals bin 1).
        k = int(p["bin"])
        fk = np.fft.fft(x[0], axis=0)[k]
        return [np.ascontiguousarray(fk.real), np.ascontiguousarray(fk.imag)]
    if fn == "scipy_uniform_filter":
        import scipy.ndimage as _sni
        return [_sni.uniform_filter(x[0], size=int(p["size"]), mode="reflect")]
    if fn == "skimage_watershed":
        # The exact call Allen Patch.split2 makes: connectivity=ones((3,3)),
        # watershed_line=False. Inputs arrive as f64; markers/mask are recast.
        import skimage.segmentation as _ss
        markers = x[1].astype(np.int32)
        mask = x[2] != 0
        out = _ss.watershed(x[0], markers, mask=mask,
                            connectivity=np.ones((3, 3)), watershed_line=False)
        return [out.astype(np.int32)]
    # --- class methods: construct the genuine object, set the inputs the method
    # reads, call the REAL method. The method body is 100% the reference's; only
    # the input-wiring is ours (as in any unit test of a method). ---
    if fn == "getRawPatchMap":
        # _getRawPatchMap thresholds signMapf at signMapThr; feed signMapf = the
        # binary mask with signMapThr=0.5 so the threshold reproduces it, then the
        # genuine open->label->per-patch-close runs. __new__ bypasses the heavy
        # __init__ (we set exactly the two attributes the method reads).
        t = RM.RetinotopicMappingTrial.__new__(RM.RetinotopicMappingTrial)
        t.signMapf = x[0]
        t.params = {"signMapThr": p["signMapThr"],
                    "openIter": int(p["openIter"]),
                    "closeIter": int(p["closeIter"])}
        return [np.asarray(t._getRawPatchMap())]
    if fn == "getDeterminantMap":
        t = RM.RetinotopicMappingTrial.__new__(RM.RetinotopicMappingTrial)
        t.altPosMapf = x[0]
        t.aziPosMapf = x[1]
        return [np.asarray(t._getDeterminantMap())]
    if fn == "getSigmaArea":
        # Patch.getSigmaArea(detMap) = sum(self.array * detMap). sign is irrelevant.
        patch = RM.Patch(x[0], 1)
        return [np.array([[float(patch.getSigmaArea(x[1]))]], dtype=np.float64)]
    if fn == "getVisualSpace":
        patch = RM.Patch(x[0], 1)
        close = int(p["closeIter"]) if "closeIter" in p else None
        vs, _uniq, _altc, _azic = patch.getVisualSpace(
            x[1], x[2], pixelSize=p.get("pixelSize", 1.0), closeIter=close
        )
        return [np.asarray(vs)]
    raise KeyError(f"unknown oracle fn {fn!r}")


def main():
    with open(sys.argv[1]) as f:
        req = json.load(f)
    x = [_load(s) for s in req["inputs"]]
    outs = dispatch(req["fn"], x, req.get("params", {}))
    meta = []
    for i, a in enumerate(outs):
        a = np.ascontiguousarray(a)
        out_path = os.path.join(req["out_dir"], f"out{i}.bin")
        a.tofile(out_path)
        meta.append({"file": out_path, "dtype": a.dtype.str, "shape": list(a.shape)})
    json.dump({"outputs": meta}, sys.stdout)


if __name__ == "__main__":
    main()
