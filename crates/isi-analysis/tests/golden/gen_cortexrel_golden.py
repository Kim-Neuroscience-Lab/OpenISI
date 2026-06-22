"""REGRESSION-LOCK for `cortex_from_reliability` (segmentation/mod.rs:102).

NO EXTERNAL ORACLE. The cross-cycle reliability *coherence*
`|Σ Z_k| / Σ|Z_k|` is a published quantity (Engel 1994; Zhuang 2017), but the
specific cortex-MASK derivation tested here — min-over-directions threshold →
largest connected component → fill holes — has no published code oracle in our
reference set. Allen `RetinotopicMapping.py` performs NO cortex restriction (it
runs full-frame). This fixture therefore pins OpenISI's OWN current behaviour
(the `min > threshold` + `is_finite` keep-rule) as an executable record; it does
NOT establish faithfulness to any external method. The cleanup uses the real
library calls OpenISI mirrors:

    - keep largest CC : scipy.ndimage.label (default struct = 4-conn cross),
                        pick the label with the most pixels.
    - fill holes      : scipy.ndimage.binary_fill_holes (default struct =
                        4-conn cross) — exactly MATLAB imfill('holes') semantics
                        that our binary_fill_holes ports.

TIE-BREAK NOTE (largest CC): our Rust uses `(1..=n).max_by_key(|i| counts[i])`.
Rust's Iterator::max_by_key returns the LAST element among equal maxima, and
label_4conn assigns labels in raster (row-major) order, so on a size tie our
Rust keeps the component with the HIGHEST label = the one whose first pixel
appears LAST in raster scan. We replicate that exact tie rule here so the
fixture is unambiguous, AND we deliberately also emit a no-tie primary case so
the main assertion never depends on the tie rule.

NaN/Inf NOTE: our Rust keeps a pixel iff `min_rel.is_finite() && min_rel >
threshold`. The reference coherence is clamped to [0,1] (never NaN), but our
public API takes arbitrary reliability maps, so we stress NaN/Inf too:
np.minimum.reduce propagates NaN (any NaN input -> NaN min), and `NaN >= thr`
is False in numpy, so NaN pixels are dropped — matching is_finite(). +Inf is
finite-False in numpy too via our explicit is_finite guard; we mirror Rust's
is_finite() rule in the oracle below.

Outputs (all uint8 / float64, C-order row-major, little-endian), NxN:
  cortexrel_azi_fwd.npy, cortexrel_azi_rev.npy,
  cortexrel_alt_fwd.npy, cortexrel_alt_rev.npy   (float64 reliability inputs)
  cortexrel_expected.npy                          (uint8 expected cortex mask)
  cortexrel_tie_*.npy                             (a separate equal-size-tie case)

Run:  python gen_cortexrel_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 48
THRESHOLD = 0.5
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def keep_largest_cc_rust(mask):
    """scipy 4-conn label, keep largest; tie -> HIGHEST label (matches Rust
    max_by_key last-of-equal-maxima with raster-order labels)."""
    labeled, n = ni.label(mask)  # default struct = 4-conn cross
    if n == 0:
        return np.zeros_like(mask, dtype=bool)
    counts = np.bincount(labeled.ravel())
    counts[0] = 0  # ignore background
    best = 0
    best_count = -1
    for lab in range(1, n + 1):  # ascending -> last wins ties == highest label
        if counts[lab] >= best_count:
            best_count = counts[lab]
            best = lab
    return labeled == best


def cortex_from_reliability_oracle(azi_fwd, azi_rev, alt_fwd, alt_rev, threshold):
    # min over the four reliability maps (np.minimum.reduce propagates NaN)
    min_rel = np.minimum.reduce([azi_fwd, azi_rev, alt_fwd, alt_rev])
    # Rust rule: keep iff min_rel.is_finite() && min_rel > threshold
    raw = np.isfinite(min_rel) & (min_rel > threshold)
    largest = keep_largest_cc_rust(raw)
    filled = ni.binary_fill_holes(largest)  # default 4-conn cross
    return raw, filled


def save_f64(arr, name):
    np.save(os.path.join(FIX, name), np.ascontiguousarray(arr.astype("<f8")))


def save_u8(arr, name):
    np.save(os.path.join(FIX, name), np.ascontiguousarray(arr.astype(np.uint8)))


def build_primary():
    """Stress: borders, a hole to fill, an orphan blob to drop, NaN/Inf pixels,
    and pixels sitting EXACTLY on the threshold (== boundary test)."""
    rng = np.random.default_rng(20260611)
    # Start each map near-reliable everywhere so min is governed by the weakest.
    azi_fwd = np.full((N, N), 0.9)
    azi_rev = np.full((N, N), 0.9)
    alt_fwd = np.full((N, N), 0.9)
    alt_rev = np.full((N, N), 0.9)

    # Background: drive one map low so min < threshold (excluded).
    azi_fwd[:, :] = 0.2

    # Main cortex blob (a big rectangle touching the top-left border to stress
    # edge handling) — set ALL four maps high here.
    for m in (azi_fwd, azi_rev, alt_fwd, alt_rev):
        m[0:30, 0:34] = 0.85

    # Carve an interior HOLE inside the blob: drop one map below threshold there
    # so the raw mask has a hole that fill_holes must close.
    azi_rev[8:14, 10:18] = 0.1

    # Punch a couple of NaN and +Inf pixels inside the blob (must be dropped by
    # is_finite, then re-filled by fill_holes since they're interior).
    alt_fwd[20:22, 20:22] = np.nan
    alt_rev[24:25, 6:8] = np.inf

    # EXACT-threshold ring: set min == THRESHOLD on a strip. With Rust's
    # `> threshold`, these are EXCLUDED (equality drops). Place it as a thin
    # column gap that, if it were included, would change the blob shape.
    # Put it OUTSIDE the main blob so it forms an orphan strip — and make it
    # exactly == threshold to prove > (not >=).
    for m in (azi_fwd, azi_rev, alt_fwd, alt_rev):
        m[40:44, 40:44] = THRESHOLD  # min == 0.5 exactly -> excluded by `>`

    # Orphan reliable blob far from the main one (must be dropped by largest-CC).
    for m in (azi_fwd, azi_rev, alt_fwd, alt_rev):
        m[38:42, 4:10] = 0.95

    return azi_fwd, azi_rev, alt_fwd, alt_rev


def build_tie():
    """Two equal-size components (no holes, no NaN) to pin the tie-break rule:
    Rust keeps the HIGHEST label == component appearing LAST in raster order.
    Component A at top-left (labeled first), component B lower-right same size
    (labeled second) -> expected mask keeps B only."""
    azi = np.full((N, N), 0.2)
    rev = np.full((N, N), 0.9)
    af = np.full((N, N), 0.9)
    ar = np.full((N, N), 0.9)
    # Two 5x5 reliable squares of identical area.
    for m in (azi, rev, af, ar):
        m[2:7, 2:7] = 0.9       # A — appears first in raster scan (lower label)
        m[30:35, 30:35] = 0.9   # B — appears later (higher label) -> kept on tie
    return azi, rev, af, ar


def main():
    # ---- primary case ----
    a, b, c, d = build_primary()
    raw, expected = cortex_from_reliability_oracle(a, b, c, d, THRESHOLD)
    save_f64(a, "cortexrel_azi_fwd.npy")
    save_f64(b, "cortexrel_azi_rev.npy")
    save_f64(c, "cortexrel_alt_fwd.npy")
    save_f64(d, "cortexrel_alt_rev.npy")
    save_u8(raw, "cortexrel_raw.npy")
    save_u8(expected, "cortexrel_expected.npy")

    # ---- tie case ----
    ta, tb, tc, td = build_tie()
    traw, texp = cortex_from_reliability_oracle(ta, tb, tc, td, THRESHOLD)
    save_f64(ta, "cortexrel_tie_azi_fwd.npy")
    save_f64(tb, "cortexrel_tie_azi_rev.npy")
    save_f64(tc, "cortexrel_tie_alt_fwd.npy")
    save_f64(td, "cortexrel_tie_alt_rev.npy")
    save_u8(texp, "cortexrel_tie_expected.npy")

    # ---- stats ----
    _, n_raw = ni.label(raw)
    print(f"N={N} threshold={THRESHOLD}")
    print(f"[primary] raw components={n_raw} raw_sum={int(raw.sum())} "
          f"expected_sum={int(expected.sum())}")
    print(f"[primary] expected bbox rows {np.where(expected.any(1))[0].min()}.."
          f"{np.where(expected.any(1))[0].max()} "
          f"cols {np.where(expected.any(0))[0].min()}.."
          f"{np.where(expected.any(0))[0].max()}")
    holes_filled = int(expected.sum() - keep_largest_cc_rust(raw).sum())
    print(f"[primary] hole pixels filled by binary_fill_holes = {holes_filled}")
    # confirm exact-threshold strip was excluded
    strip_in_expected = int(expected[40:44, 40:44].sum())
    print(f"[primary] exact-threshold strip pixels in expected (must be 0) = {strip_in_expected}")

    _, n_traw = ni.label(traw)
    keepsA = bool(texp[2:7, 2:7].any())
    keepsB = bool(texp[30:35, 30:35].any())
    print(f"[tie] raw components={n_traw} tie_expected_sum={int(texp.sum())} "
          f"keepsA={keepsA} keepsB={keepsB} (expect keepsA=False keepsB=True)")


if __name__ == "__main__":
    main()
