"""Golden for `patches_from_labels_majority_sign`
(segmentation/connectivity.rs:207) against a VERBATIM transcription of SNLC
`getPatchSign.m` (reference/ISI/getPatchSign.m).

getPatchSign.m (MATLAB), verbatim:
    imlabel = bwlabel(im,4);
    areaID = unique(imlabel);          % sorted ascending: [0,1,2,...,N]
    patchSign = zeros(size(imlabel));
    for i = 2:length(areaID)           % skip areaID(1)==0 (background)
       id = find(imlabel == areaID(i));
       m = mean(imsign(id));
       areaSign(i-1) = sign(m);        % MATLAB sign: -1 / 0 / +1
       patchSign(id) = sign(m)+1.1;    % encode: -1->0.1, 0->1.1, +1->2.1
    end

Our Rust takes an ALREADY-LABELLED map (label IDs preserved through
dilation_patches2), then for each label k in 1..=N computes mean(signal over
label k) and assigns sign = (sum >= 0.0) ? +1 : -1. So the oracle's bwlabel step
is replaced by a precomputed label map; this golden therefore supplies a label
map directly and checks the PER-LABEL SIGN, which is the only thing our function
derives. We transcribe getPatchSign's m=mean(...); areaSign=sign(m) faithfully.

We deliberately stress:
  - label IDs that are NOT a dense 1..N from a fresh bwlabel (preserved IDs),
    but our Rust iterates k in 1..=n so we keep them dense 1..N here; the
    important variable is the SIGN, which is label-id-independent.
  - mixed-sign pixels within a label (majority vote by mean, not pixel sign)
  - a label whose mean is EXACTLY 0.0 (the tie). MATLAB sign(0)=0 -> encoded
    1.1 (neither + nor -). Our Rust forces +1 here. We record the oracle's TRUE
    value (0) so the comparison surfaces the divergence.
  - small magnitudes near zero, large mixed magnitudes
  - a label far from origin / touching the border

Output fixtures (NxN, C-order row-major, little-endian):
  psign_labels.bin  : int32   label map (0=bg, 1..N)
  psign_signal.bin  : float64 signal (smoothed VFS surrogate)
  psign_n.bin       : int32   scalar N (number of labels)
  psign_expsign.bin : int32   expected MATLAB sign per label, index k-1 -> label k
                              values in {-1,0,1}
Run: python gen_patchsign_majority_golden.py
"""
import os
import numpy as np

N = 32
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def matlab_sign(x):
    # MATLAB sign(): -1 if x<0, 0 if x==0, +1 if x>0
    if x > 0:
        return 1
    if x < 0:
        return -1
    return 0


def get_patch_sign_per_label(imlabel, imsign):
    """Verbatim getPatchSign.m sign logic. Returns dict label->sign and
    list of (label, sign) for labels present, ascending."""
    areaID = np.unique(imlabel)              # sorted ascending incl. 0
    out = {}
    for i in range(1, len(areaID)):          # MATLAB i=2:end -> skip areaID[0]==0... but
        lab = areaID[i]                      # only if areaID[0]==0; handle generally below
        ids = imlabel == lab
        m = float(np.mean(imsign[ids]))      # mean(imsign(id))
        out[int(lab)] = matlab_sign(m)
    # If background label 0 was absent, the i=2:end loop in MATLAB would skip the
    # FIRST present label. But a real label map always contains 0 (background),
    # which our construction guarantees, so areaID[0]==0 and we correctly skip it.
    return out


def main():
    labels = np.zeros((N, N), dtype=np.int32)
    signal = np.zeros((N, N), dtype=np.float64)

    # --- Label 1: clearly positive mean (all positive pixels) ---
    labels[2:6, 2:6] = 1
    signal[2:6, 2:6] = 0.7

    # --- Label 2: clearly negative mean ---
    labels[2:6, 10:14] = 2
    signal[2:6, 10:14] = -0.9

    # --- Label 3: MIXED signs, mean still POSITIVE (majority-by-mean test) ---
    labels[10:14, 2:6] = 3
    sig3 = signal[10:14, 2:6]
    sig3[:] = 0.5
    sig3[0, 0] = -0.3      # a couple of opposite-sign pixels
    sig3[0, 1] = -0.2
    signal[10:14, 2:6] = sig3

    # --- Label 4: MIXED signs, mean NEGATIVE ---
    labels[10:14, 10:14] = 4
    sig4 = signal[10:14, 10:14]
    sig4[:] = -0.4
    sig4[0, 0] = 0.9
    sig4[0, 1] = 0.8
    signal[10:14, 10:14] = sig4

    # --- Label 5: mean EXACTLY 0.0 (the tie). 8 pixels: +0.5 x4, -0.5 x4 ---
    labels[18:20, 2:6] = 5
    blk = signal[18:20, 2:6]
    blk[0, :] = 0.5
    blk[1, :] = -0.5
    signal[18:20, 2:6] = blk

    # --- Label 6: tiny positive mean (near-zero but > 0) ---
    labels[18:20, 10:14] = 6
    signal[18:20, 10:14] = 1e-9

    # --- Label 7: single pixel, positive, on the BORDER (row 0) ---
    labels[0, N - 1] = 7
    signal[0, N - 1] = 0.3

    # --- Label 8: touches border (last row), negative ---
    labels[N - 1, 4:8] = 8
    signal[N - 1, 4:8] = -0.6

    n = int(labels.max())
    assert n == 8

    persign = get_patch_sign_per_label(labels, signal)
    expsign = np.array([persign[k] for k in range(1, n + 1)], dtype=np.int32)

    np.ascontiguousarray(labels.astype("<i4")).tofile(os.path.join(FIX, "psign_labels.bin"))
    np.ascontiguousarray(signal.astype("<f8")).tofile(os.path.join(FIX, "psign_signal.bin"))
    np.array([n], dtype="<i4").tofile(os.path.join(FIX, "psign_n.bin"))
    expsign.astype("<i4").tofile(os.path.join(FIX, "psign_expsign.bin"))

    print(f"  grid N={N}, n_labels={n}")
    print(f"  signal sum={signal.sum():.6f}  min={signal.min():.6f}  max={signal.max():.6f}")
    for k in range(1, n + 1):
        ids = labels == k
        m = float(np.mean(signal[ids]))
        print(f"  label {k}: px={int(ids.sum()):3d}  mean={m:+.6e}  matlab_sign={persign[k]:+d}")
    print(f"  expected signs (label1..N): {list(expsign)}")
    print(f"  NOTE label5 mean==0 -> MATLAB sign=0 ; Rust (sum>=0) forces +1  (DIVERGENCE point)")


if __name__ == "__main__":
    main()
