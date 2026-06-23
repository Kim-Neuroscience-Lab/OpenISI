% End-to-end golden for the SNLC cortex method (getMouseAreasX.m sequence):
%   thr = k*std(VFS)*0.5  (k=1.5)  ->  |VFS|>thr  ->  imopen(disk2)
%   -> imclose(disk10) -> imfill -> imdilate(disk3) -> imfill -> largest 4-CC
%
% Validates the *orchestration* on top of the per-op golden
% (gen_cortex_morph_golden.m). Input is a |VFS| amplitude field that is high
% (1.0) inside cortex-like shapes and low (0.05) in background, with a wide
% margin around the threshold so the std-convention question (N vs N-1, see
% below) cannot flip any pixel — this test isolates threshold+morphology, not
% the std estimator.
%
% Output: fixtures/cortex_full_vfs.npy (float64 96x96),
%         fixtures/cortex_full_golden.npy (uint8 96x96)
%
% Run:  matlab -batch gen_cortex_full_golden   (via `cargo xtask goldens cortex_full`)
% IPT (imopen/imclose/imdilate/imfill/bwlabel) is built into MATLAB — no pkg load.

addpath(fileparts(mfilename('fullpath')));  % for save_npy

N = 96;
[X, Y] = meshgrid(1:N, 1:N);
shape = false(N, N);
shape = shape | (((X - 30).^2 + (Y - 50).^2) <= 18^2);
shape(((X - 30).^2 + (Y - 50).^2) <= 6^2) = false;     % hole
shape = shape | (((X - 62).^2 + (Y - 2).^2) <= 12^2);  % top-border blob
shape = shape | (((X - 72).^2 + (Y - 64).^2) <= 6^2);
shape = shape | (((X - 72).^2 + (Y - 82).^2) <= 6^2);
shape(8, 88) = true; shape(88, 12) = true; shape(4, 4) = true;

VFS = 0.05 * ones(N, N);
VFS(shape) = 1.0;          % |VFS| high in cortex, low background

k = 1.5;
s_n1 = std(VFS(:));        % default: sample std (N-1) — what MATLAB/SNLC uses
s_n  = std(VFS(:), 1);     % population std (N) — what our Rust currently uses
thr  = k * s_n1 * 0.5;

imseg  = abs(VFS) > thr;
op     = imopen(imseg, strel('disk', 2, 0));
cl     = imclose(op, strel('disk', 10, 0));
fl     = imfill(cl, 'holes');
di     = imdilate(fl, strel('disk', 3, 0));
fl2    = imfill(di, 'holes');
lbl    = bwlabel(fl2, 4);
nlab   = max(lbl(:));
counts = zeros(nlab, 1);
for i = 1:nlab; counts(i) = sum(lbl(:) == i); end
[~, big] = max(counts);
cortex = (lbl == big);

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
save_npy(fullfile(fixdir, 'cortex_full_vfs.npy'),    VFS,           '<f8');
save_npy(fullfile(fixdir, 'cortex_full_golden.npy'), uint8(cortex), '|u1');

fprintf('  std N-1=%.6f  N=%.6f  (ratio %.6f)\n', s_n1, s_n, s_n1 / s_n);
fprintf('  thr(N-1)=%.6f  thr(N)=%.6f  cortex_sum=%d\n', thr, k * s_n * 0.5, sum(cortex(:)));
fprintf('  (margin: VFS values are 0.05 and 1.0; threshold ~%.2f, no borderline px)\n', thr);
