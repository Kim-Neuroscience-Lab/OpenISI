"""Render the ΔF/F-baseline comparison from baseline_compare bins.

4 rows x 4 cols; columns are the four baseline methods, and EACH ROW is a
single quantity with ONE shared, properly-scaled colorbar:
  Row 0 — per-pixel F0 baseline map.
  Row 1 — F1 ΔF/F amplitude under that baseline.
  Row 2 — F0 difference vs the production default (this method − Allen all-frame
          mean). Diverging scale; col 0 is the reference (≡0).
  Row 3 — F1 amplitude difference vs the default.
The two difference rows localize WHERE the baseline choice matters: the point of
the inter-sweep method is that the all-frame baseline is contaminated by
stimulus-driven (e.g. aperture-locked) activity, which shows up here.
"""
import os
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

H = W = 512
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..")
D = os.path.join(ROOT, "target", "baseline_compare")

METHODS = ["allen_mean", "allen_median", "inter_mean", "inter_median"]
TITLES = {
    "allen_mean": "Allen all-frame MEAN\n(production default)",
    "allen_median": "Allen all-frame MEDIAN",
    "inter_mean": "OpenISI inter-sweep MEAN",
    "inter_median": "OpenISI inter-sweep MEDIAN",
}
ROW_LABELS = ["F0 baseline", "F1 ΔF/F amplitude", "F0 − default", "|F1| − default"]


def load(kind, name):
    return np.fromfile(os.path.join(D, f"{kind}_{name}.bin"), dtype="<f8").reshape(H, W)


def draw_row(fig, axrow, imgs, cmap, vmin, vmax, label, cbar_label):
    im = None
    for a, img in zip(axrow, imgs):
        im = a.imshow(img, cmap=cmap, vmin=vmin, vmax=vmax, interpolation="nearest")
        a.set_xticks([]); a.set_yticks([])
    axrow[0].set_ylabel(label, fontsize=12)
    cb = fig.colorbar(im, ax=list(axrow), fraction=0.024, pad=0.012)
    cb.set_label(cbar_label, fontsize=9)


def main():
    f0 = {m: load("f0", m) for m in METHODS}
    amp = {m: load("amp", m) for m in METHODS}
    base = "allen_mean"
    f0_diff = {m: f0[m] - f0[base] for m in METHODS}
    amp_diff = {m: amp[m] - amp[base] for m in METHODS}

    # Per-row shared scales (robust percentiles).
    f0_lo, f0_hi = np.percentile(np.stack(list(f0.values())), [2, 98])
    amp_hi = np.percentile(np.stack(list(amp.values())), 99)
    f0d = np.percentile(np.abs(np.stack([f0_diff[m] for m in METHODS[1:]])), 99) or 1.0
    ampd = np.percentile(np.abs(np.stack([amp_diff[m] for m in METHODS[1:]])), 99) or 1.0

    fig, ax = plt.subplots(4, 4, figsize=(17, 17))
    for j, m in enumerate(METHODS):
        ax[0, j].set_title(TITLES[m], fontsize=11)

    draw_row(fig, ax[0], [f0[m] for m in METHODS], "viridis", f0_lo, f0_hi,
             ROW_LABELS[0], "counts")
    draw_row(fig, ax[1], [amp[m] for m in METHODS], "magma", 0.0, amp_hi,
             ROW_LABELS[1], "ΔF/F")
    draw_row(fig, ax[2], [f0_diff[m] for m in METHODS], "RdBu_r", -f0d, f0d,
             ROW_LABELS[2], "Δ counts")
    draw_row(fig, ax[3], [amp_diff[m] for m in METHODS], "RdBu_r", -ampd, ampd,
             ROW_LABELS[3], "Δ ΔF/F")

    fig.suptitle(
        "ΔF/F baseline comparison — F0, F1 amplitude, and difference vs the all-frame default",
        fontsize=15,
    )
    out = os.path.join(D, "baseline_compare.png")
    fig.savefig(out, dpi=150, bbox_inches="tight")
    print(f"  wrote {out}")


if __name__ == "__main__":
    main()
