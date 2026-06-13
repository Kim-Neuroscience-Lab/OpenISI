"""Render the signal-quality-mask comparison from sq_mask_demo bins.
Top-left: unmasked smoothed VFS. Then the VFS masked by each metric
(reliability / SNR / F1 amplitude), each keeping the same pixel fraction.
Masked-out pixels are transparent (shown light grey)."""
import os
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

H = W = 512
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..")
D = os.path.join(ROOT, "target", "sq_mask_demo")


def load(name):
    return np.fromfile(os.path.join(D, name), dtype="<f8").reshape(H, W)


def main():
    panels = [
        ("Reliability (>=0.85)", load("masked_reliability.bin")),
        ("Allen spectral power-SNR (sigma=1)", load("masked_allen.bin")),
        ("Spectral SNR (matched %)", load("masked_snr.bin")),
        ("F1 amplitude (matched %)", load("masked_amplitude.bin")),
    ]
    cmap = plt.cm.RdBu.copy()
    cmap.set_bad(color="0.85")  # NaN (masked-out) -> light grey
    fig, ax = plt.subplots(2, 2, figsize=(9, 9))
    for a, (title, img) in zip(ax.ravel(), panels):
        a.imshow(np.ma.masked_invalid(img), cmap=cmap, vmin=-1, vmax=1, interpolation="nearest")
        a.set_title(title, fontsize=10)
        a.set_xticks([]); a.set_yticks([])
    fig.suptitle("Signal-quality masks applied to the smoothed VFS (6_4)", fontsize=12)
    fig.tight_layout(rect=[0, 0, 1, 0.96])
    out = os.path.join(D, "sq_mask_compare.png")
    fig.savefig(out, dpi=110)
    print(f"  wrote {out}")


if __name__ == "__main__":
    main()
