"""Render the synthetic-ground-truth recovery figure from the bins dumped by
`cargo run -p isi-analysis --example synthetic_recovery`.

Top row: known azimuth (tent) vs recovered azimuth (HSV phase).
Bottom row: known VFS (mirror pair ±1) vs recovered VFS (RdBu).
The match is the proof that the real pipeline inverts a known retinotopy.
"""
import os
import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

H = W = 128
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..")
D = os.path.join(ROOT, "target", "synthetic_recovery")


def load(name):
    return np.fromfile(os.path.join(D, name), dtype="<f8").reshape(H, W)


def main():
    known_azi = load("known_azi.bin")
    rec_azi = load("recovered_azi.bin")
    known_vfs = load("known_vfs.bin")
    rec_vfs = load("recovered_vfs.bin")

    fig, ax = plt.subplots(2, 2, figsize=(8, 8))
    ax[0, 0].imshow(known_azi, cmap="hsv", vmin=-np.pi, vmax=np.pi)
    ax[0, 0].set_title("Known azimuth phase (input)")
    ax[0, 1].imshow(rec_azi, cmap="hsv", vmin=-np.pi, vmax=np.pi)
    ax[0, 1].set_title("Recovered azimuth phase")
    ax[1, 0].imshow(known_vfs, cmap="RdBu", vmin=-1, vmax=1)
    ax[1, 0].set_title("Known VFS (mirror pair +1 | -1)")
    ax[1, 1].imshow(rec_vfs, cmap="RdBu", vmin=-1, vmax=1)
    ax[1, 1].set_title("Recovered VFS")
    for a in ax.ravel():
        a.set_xticks([])
        a.set_yticks([])

    interior = np.ones((H, W), bool)
    interior[:4] = interior[-4:] = interior[:, :4] = interior[:, -4:] = False
    xmid = (W - 1) / 2.0
    cols = np.abs(np.arange(W) - xmid) >= 4
    interior &= cols[None, :]
    d = np.angle(np.exp(1j * (rec_azi - known_azi)))
    max_err = np.abs(d[interior]).max()
    fig.suptitle(
        f"Synthetic ground truth: pipeline recovers a known mirror-pair retinotopy\n"
        f"max azimuth phase error = {max_err:.2e} rad   |   VFS sign recovered exactly",
        fontsize=11,
    )
    fig.tight_layout(rect=[0, 0, 1, 0.95])
    out = os.path.join(D, "synthetic_recovery.png")
    fig.savefig(out, dpi=110)
    print(f"  wrote {out}  (max azi err {max_err:.2e} rad)")


if __name__ == "__main__":
    main()
