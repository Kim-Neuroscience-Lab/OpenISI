% Golden for SNLC `getPatchCoM` — per-patch center-of-mass (pixel coords). Used
% by the Garrett refinement to locate V1's center (the visual position AT the
% pixel-centroid of the largest patch), which differs from OpenISI's mean-based
% `v1_center` — so it must be ported faithfully.
%
% `getPatchCoM.m` is a standalone oracle file with NO plotting, so this calls the
% REAL oracle directly (addpath reference/ISI) — no transcription. The only thing
% pinned here is `CoMxy` (the center of mass per patch); `Axisxy` (principal
% axis) is computed by the oracle but unused by the split/fuse path.
%
% Synthetic: three 4-connected patches of distinct size/shape, including a
% C-shaped (curved) one whose centroid falls OFF the patch — exercising the
% oracle's "snap to nearest patch pixel" correction.
%
% Outputs (fixtures/, row-major):
%   patchcom_im.bin     (H x W uint8) the patch image (0/1, multiple components)
%   patchcom_comxy.bin  (Npatch x 2 f64) CoMxy: column 1 = x (col), 2 = y (row), 1-based
%   patchcom_meta.bin   (f64) [H, W, Npatch]
% Run:  via `cargo xtask goldens patchcom`

pkg load image;
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));   % the real SNLC oracle source

H = 80; W = 80;
im = zeros(H, W);
% Patch 1 — large filled rectangle (V1-like, biggest).
im(10:45, 10:40) = 1;
% Patch 2 — small filled square.
im(60:70, 15:25) = 1;
% Patch 3 — C-shape (curved): centroid lies in the hollow, off the patch.
im(12:40, 55:72) = 1;
im(20:32, 55:66) = 0;       % carve the notch open to the right → C opening

[CoMxy, Axisxy] = getPatchCoM(im);   % REAL oracle
npatch = size(CoMxy, 1);

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'patchcom_im.bin'), 'w'); fwrite(fid, uint8(im'), 'uint8'); fclose(fid);
% CoMxy is Npatch x 2; write row-major so Rust reads [p][0]=x, [p][1]=y.
fid = fopen(fullfile(fixdir, 'patchcom_comxy.bin'), 'w'); fwrite(fid, CoMxy', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'patchcom_meta.bin'), 'w'); fwrite(fid, [H; W; npatch], 'double'); fclose(fid);

printf('  patchcom: %d patches; CoMxy=', npatch);
for i = 1:npatch
  printf('(%.2f,%.2f) ', CoMxy(i,1), CoMxy(i,2));
end
printf('\n');
