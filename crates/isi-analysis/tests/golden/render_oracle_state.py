"""Render the **state of the oracle / regression cross-validation**.

Two dataset paths, each dumped to `target/oracle_state/<dataset>/` by
`cargo run -p isi-analysis --example oracle_state`:

  * **synthetic** — each method on its per-op golden fixture vs the verbatim
    reference output (column 1 = a true external oracle: Allen/SNLC/numpy/scipy).
  * **r43** — the full pipeline re-run on the real R43_smoke recording, every
    `/results` leaf vs the committed baseline (column 1 = the reference).

This script renders *whichever* datasets are present: one figure per
(dataset, group), `target/oracle_state/<dataset>_<group>_oracle_state.png`,
overwritten each run. Each figure is a grid — rows = methods/leaves in
pipeline-DAG order, columns = [oracle|reference | OpenISI | difference].

The wrapper `cargo xtask figures oracle_state` runs the dump example first.

Heterogeneity is handled by colormaps keyed on each panel's `kind`:
  - periodic  (phase 2π, axis 180°, polar 360°) → cyclic `twilight`; diff is
    wrap-aware. The display range comes from the manifest.
  - diverging (signed, ~0-centred)              → `RdBu_r`, symmetric limits.
  - sequential(positive magnitude)              → `viridis`, robust 2–98 %ile.
  - mask      (boolean)                         → `gray`; diff is the 4-way
    {both, oracle-only, ours-only, neither} categorical map.
  - labels    (integer area ids)                → qualitative `tab20`; diff
    highlights pixels whose label differs.

Style: Bold Arial, 2-pt axes + ticks, with the min/max tick at each axis edge.
"""
import glob
import json
import os

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import ListedColormap, BoundaryNorm
from matplotlib.patches import Patch

matplotlib.rcParams.update({
    "font.family": "sans-serif",
    "font.sans-serif": ["Arial", "Liberation Sans", "DejaVu Sans"],
    "font.weight": "bold",
    "axes.linewidth": 2.0,
    "axes.titleweight": "bold",
    "axes.labelweight": "bold",
    "xtick.major.width": 2.0,
    "ytick.major.width": 2.0,
    "xtick.major.size": 5.0,
    "ytick.major.size": 5.0,
})

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..")
BASE = os.path.join(ROOT, "target", "oracle_state")

# Boolean-mask diff: index = 2*oracle + ours.
#   0 neither (black) | 1 ours-only (red) | 2 oracle-only (blue) | 3 both (white)
MASK_CMAP = ListedColormap(["#000000", "#d62728", "#1f77b4", "#ffffff"])
MASK_NORM = BoundaryNorm([-0.5, 0.5, 1.5, 2.5, 3.5], MASK_CMAP.N)
# Label diff: same (white) vs differ (red).
LABELDIFF_CMAP = ListedColormap(["#ffffff", "#d62728"])
LABELDIFF_NORM = BoundaryNorm([-0.5, 0.5, 1.5], LABELDIFF_CMAP.N)


def load(d, name, h, w):
    return np.fromfile(os.path.join(d, name), dtype="<f8").reshape(h, w)


def edge_ticks(ax, h, w):
    ax.set_xticks([0, w - 1])
    ax.set_yticks([0, h - 1])
    ax.tick_params(labelsize=8)


def wrap(d, period):
    return (d + period / 2.0) % period - period / 2.0


def robust(values, lo=2, hi=98):
    finite = values[np.isfinite(values)]
    if finite.size == 0:
        return 0.0, 1.0
    return float(np.percentile(finite, lo)), float(np.percentile(finite, hi))


def value_limits(oracle, ours, p):
    """Shared scale for the oracle and OpenISI columns (so they're comparable)."""
    kind = p["kind"]
    both = np.concatenate([oracle.ravel(), ours.ravel()])
    if kind == "periodic":
        return p.get("vmin", -np.pi), p.get("vmax", np.pi)
    if kind == "mask":
        return 0.0, 1.0
    if kind == "labels":
        finite = both[np.isfinite(both)]
        return 0.0, float(finite.max()) if finite.size else 1.0
    if kind == "diverging":
        lo, hi = robust(np.abs(both))
        lim = max(abs(lo), abs(hi))
        return -lim, (lim if lim > 0 else 1.0)
    return robust(both)  # sequential


def show_value(ax, img, p, vmin, vmax):
    kind = p["kind"]
    if kind == "periodic":
        return ax.imshow(img, cmap="twilight", vmin=vmin, vmax=vmax)
    if kind == "diverging":
        return ax.imshow(img, cmap="RdBu_r", vmin=vmin, vmax=vmax)
    if kind == "mask":
        return ax.imshow(img, cmap="gray", vmin=0, vmax=1)
    if kind == "labels":
        return ax.imshow(img, cmap="tab20", vmin=vmin, vmax=vmax, interpolation="nearest")
    return ax.imshow(img, cmap="viridis", vmin=vmin, vmax=vmax)


def show_diff(ax, oracle, ours, p):
    """Render the difference column; return (image_or_None, summary string)."""
    kind = p["kind"]
    if kind == "mask":
        idx = 2 * np.rint(oracle).astype(int) + np.rint(ours).astype(int)
        im = ax.imshow(idx, cmap=MASK_CMAP, norm=MASK_NORM)
        n = int(np.count_nonzero((idx == 1) | (idx == 2)))
        return im, f"mismatch: {n} px"
    if kind == "labels":
        differ = (np.rint(oracle) != np.rint(ours)).astype(int)
        im = ax.imshow(differ, cmap=LABELDIFF_CMAP, norm=LABELDIFF_NORM)
        return im, f"label diffs: {int(differ.sum())} px"
    if kind == "periodic":
        d = wrap(ours - oracle, p["period"])
    else:
        d = ours - oracle
    lim = float(np.nanmax(np.abs(d))) if np.isfinite(d).any() else 0.0
    im = ax.imshow(d, cmap="RdBu_r", vmin=-(lim or 1.0), vmax=(lim or 1.0))
    return im, f"max|Δ|: {lim:.2e}"


def render_group(d, dataset, col1, caption, group, panels):
    panels = sorted(panels, key=lambda p: p["order"])
    n = len(panels)
    fig, axes = plt.subplots(n, 3, figsize=(9.5, 2.7 * n + 0.6), squeeze=False)

    for row, p in enumerate(panels):
        h, w = p["h"], p["w"]
        oracle = load(d, f"{p['name']}.oracle.bin", h, w)
        ours = load(d, f"{p['name']}.ours.bin", h, w)
        vmin, vmax = value_limits(oracle, ours, p)

        ax_o, ax_u, ax_d = axes[row]
        im_o = show_value(ax_o, oracle, p, vmin, vmax)
        show_value(ax_u, ours, p, vmin, vmax)
        im_d, summary = show_diff(ax_d, oracle, ours, p)

        # Shared colorbar on OpenISI col; diff colorbar on diff col (skip the
        # categorical mask/label diffs — their colors are a discrete legend).
        if p["kind"] not in ("mask", "labels"):
            fig.colorbar(im_o, ax=ax_u, fraction=0.046, pad=0.04)
            fig.colorbar(im_d, ax=ax_d, fraction=0.046, pad=0.04)
        else:
            fig.colorbar(im_o, ax=ax_u, fraction=0.046, pad=0.04)

        for ax in (ax_o, ax_u, ax_d):
            edge_ticks(ax, h, w)
        ax_o.set_ylabel(p["title"], fontsize=9)
        ax_d.set_title(summary, fontsize=9, color="0.25")
        ax_o.text(0.5, -0.12, p["oracle_ref"], transform=ax_o.transAxes,
                  ha="center", va="top", fontsize=7, color="0.35", weight="normal")
        if row == 0:
            ax_o.set_title(col1, fontsize=12)
            ax_u.set_title("OpenISI", fontsize=12)

    if any(p["kind"] == "mask" for p in panels):
        legend = [
            Patch(facecolor="#ffffff", edgecolor="0.5", label="both"),
            Patch(facecolor="#1f77b4", label=f"{col1} only"),
            Patch(facecolor="#d62728", label="OpenISI only"),
            Patch(facecolor="#000000", label="neither"),
        ]
        fig.legend(handles=legend, loc="lower center", ncol=4, fontsize=8,
                   frameon=False, bbox_to_anchor=(0.5, 0.0))

    title = f"{dataset} · {group}"
    if caption:
        fig.suptitle(f"{title}\n{caption}", fontsize=12)
    else:
        fig.suptitle(title, fontsize=14)
    fig.tight_layout(rect=[0, 0.02, 1, 0.96])
    out = os.path.join(BASE, f"{dataset}_{group}_oracle_state.png")
    fig.savefig(out, dpi=130)
    plt.close(fig)
    print(f"  wrote {out}  ({n} rows)")


def render_dataset(manifest_path):
    d = os.path.dirname(manifest_path)
    with open(manifest_path, encoding="utf-8") as f:
        m = json.load(f)
    dataset = m["dataset"]
    col1 = m.get("col1", "oracle")
    caption = m.get("caption", "")
    groups = {}
    for p in m["panels"]:
        groups.setdefault(p["group"], []).append(p)
    for group in sorted(groups):
        render_group(d, dataset, col1, caption, group, groups[group])


def main():
    manifests = sorted(glob.glob(os.path.join(BASE, "*", "manifest.json")))
    if not manifests:
        raise SystemExit(
            f"no manifests under {BASE}\n"
            "run `cargo run -p isi-analysis --example oracle_state` first "
            "(or `cargo xtask figures oracle_state`)."
        )
    for mp in manifests:
        render_dataset(mp)


if __name__ == "__main__":
    main()
