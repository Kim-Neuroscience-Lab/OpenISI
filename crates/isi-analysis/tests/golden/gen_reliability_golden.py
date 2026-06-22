"""Golden for `compute_reliability` (compute/ops.rs:368) — the cross-cycle
coherence metric the OpenIsiCrossCycleReliability signal-quality mask thresholds.

The metric is the Engel 1994 / Zhuang 2017 (eLife 6:e18372) vector coherence
across cycles, per pixel:

    reliability = | Σ_k Z_k |  /  Σ_k | Z_k |        ∈ [0, 1]

where Z_k is the cycle-k complex F1 projection. 1.0 = every cycle's phasor
points the same way (perfectly repeatable); → 0 = phasors cancel (noise). There
is no single reference *codebase* for this (it is a published formula), so the
oracle is a verbatim numpy transcription of the formula — exactly how we pinned
the documented `compute_snr` rule.

Synthetic input: a COHERENT region (all cycles share a phase, tiny jitter →
reliability ≈ 1) and an INCOHERENT region (random per-cycle phases → low
reliability), with varying amplitudes so the amplitude-weighting is exercised.

Inputs stored as f32 (the device `Complex2` precision the Rust op consumes);
expected reliability in f64. Compare with f32 tolerance.

Output: fixtures/rel_z_re.npy, rel_z_im.npy  (f32 [K,H,W] row-major)
        fixtures/rel_expected.npy             (f64 [H,W])
Run: python gen_reliability_golden.py
"""
import os
import numpy as np

K, H, W = 5, 8, 8
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def main():
    rng = np.random.default_rng(20260611)
    z = np.zeros((K, H, W), dtype=np.complex128)
    for r in range(H):
        for c in range(W):
            if c < 4:
                # Coherent: shared phase across cycles + small jitter; amplitude
                # varies across pixels and cycles (amplitude-weighting test).
                phi = 0.4 * (r - c)
                for k in range(K):
                    amp = 1.0 + 0.1 * k + 0.05 * r
                    z[k, r, c] = amp * np.exp(1j * (phi + 0.04 * rng.standard_normal()))
            else:
                # Incoherent: independent random phases per cycle.
                for k in range(K):
                    amp = 0.5 + 0.5 * rng.random()
                    z[k, r, c] = amp * np.exp(1j * rng.uniform(-np.pi, np.pi))

    # --- verbatim metric ---
    num = np.abs(np.sum(z, axis=0))            # |Σ_k Z_k|
    denom = np.sum(np.abs(z), axis=0)          # Σ_k |Z_k|
    rel = num / np.maximum(denom, 1e-20)
    # --- end ---

    z32 = z.astype(np.complex64)
    np.save(os.path.join(FIX, "rel_z_re.npy"), np.ascontiguousarray(z32.real, dtype="<f4"))
    np.save(os.path.join(FIX, "rel_z_im.npy"), np.ascontiguousarray(z32.imag, dtype="<f4"))
    np.save(os.path.join(FIX, "rel_expected.npy"), np.ascontiguousarray(rel, dtype="<f8"))

    print(f"  K={K} H={H} W={W}")
    print(f"  coherent region (c<4) reliability range  [{rel[:,:4].min():.4f}, {rel[:,:4].max():.4f}]")
    print(f"  incoherent region (c>=4) reliability range [{rel[:,4:].min():.4f}, {rel[:,4:].max():.4f}]")


if __name__ == "__main__":
    main()
