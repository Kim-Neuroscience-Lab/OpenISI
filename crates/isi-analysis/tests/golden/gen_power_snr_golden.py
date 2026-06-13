"""Golden for the Allen spectral-power signal mask, against a VERBATIM
transcription of `corticalmapping/RetinotopicMapping.py::generatePhaseMap`
(the power-thresholding branch, L169-185):

    spectrumMovie = np.fft.fft(movie, axis=0)
    powerMovie    = (np.abs(spectrumMovie) * 2.) / np.size(movie, 0)
    powerMap      = np.abs(powerMovie[cycles, :, :])          # power at F1
    meanPower     = np.mean(powerMovie, axis=0)               # over ALL freqs
    stdPower      = np.std(powerMovie, axis=0)                # population (ddof=0)
    # pixel kept iff:  powerMap >= meanPower + sigma * stdPower   (else phase->NaN)

i.e. a per-pixel responsiveness mask: keep a pixel iff its power at the stimulus
frequency exceeds its own broadband noise floor by `sigma` standard deviations.
No connected-component / fill cleanup — it is a raw per-pixel mask (that is the
faithful Allen behaviour; region cleanup is a separate, downstream concern).

Synthetic movie (n cycles of stimulus): a responsive block (strong F1 tone) and
a noise block (no F1), plus a graded-amplitude strip so the threshold boundary
is exercised, plus broadband noise everywhere. `np.std` default ddof=0 — our
Rust must use POPULATION std to match.

Output: fixtures/powersnr_movie.bin (f32 [n,H,W] row-major),
        fixtures/powersnr_mask.bin (u8 [H,W], 1 = kept/responsive)
Run: python gen_power_snr_golden.py
"""
import os
import numpy as np

N, H, W = 24, 16, 16
CYCLES = 4          # 4 stimulus cycles in the movie -> F1 at FFT bin 4
SIGMA = 1.0
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def allen_power_mask(movie, cycles, sigma):
    # --- verbatim generatePhaseMap power branch (RetinotopicMapping.py L169-185) ---
    spectrumMovie = np.fft.fft(movie, axis=0)
    powerMovie = (np.abs(spectrumMovie) * 2.0) / np.size(movie, 0)
    powerMap = np.abs(powerMovie[cycles, :, :])
    meanPower = np.mean(powerMovie, axis=0)
    stdPower = np.std(powerMovie, axis=0)          # ddof=0 (population)
    keep = powerMap >= (meanPower + sigma * stdPower)
    return keep
    # --- end verbatim criterion ---


def main():
    rng = np.random.default_rng(20260611)
    t = np.arange(N)
    f1 = np.cos(2.0 * np.pi * CYCLES * t / N)        # unit F1 tone

    movie = 0.30 * rng.standard_normal((N, H, W))    # broadband noise everywhere

    # Responsive block (rows 0..8, cols 0..8): strong F1.
    for r in range(0, 8):
        for c in range(0, 8):
            movie[:, r, c] += (1.0 + 0.05 * (r + c)) * f1

    # Graded strip (rows 0..8, cols 8..12): F1 amplitude ramps down toward zero
    # so some pixels sit just above / below the keep threshold.
    for r in range(0, 8):
        for c in range(8, 12):
            amp = 0.6 * (12 - c) / 4.0
            movie[:, r, c] += amp * f1

    # Rows 8..16: pure noise (no F1) -> must be dropped.

    movie = movie.astype(np.float32)
    keep = allen_power_mask(movie.astype(np.float64), CYCLES, SIGMA)

    np.ascontiguousarray(movie, dtype="<f4").tofile(os.path.join(FIX, "powersnr_movie.bin"))
    np.ascontiguousarray(keep.astype(np.uint8)).tofile(os.path.join(FIX, "powersnr_mask.bin"))

    print(f"  N={N} H={H} W={W} cycles={CYCLES} sigma={SIGMA}")
    print(f"  kept pixels = {int(keep.sum())}/{H*W}")
    print(f"  responsive block kept = {int(keep[0:8,0:8].sum())}/64 (expect ~64)")
    print(f"  noise rows kept = {int(keep[8:16,:].sum())} (expect ~0)")


if __name__ == "__main__":
    main()
