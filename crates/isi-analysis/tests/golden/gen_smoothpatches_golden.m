% Golden for SNLC `smoothPatchesX(map, im)` — per-patch Gaussian smoothing of an
% eccentricity map (σ scaled by patch area), used by splitPatchesX before the
% local-minima count. Background (outside any patch) is set to 45; each patch is
% filtered with `fspecial('gaussian', size, area/2000)` via `roifilt2`
% (= `filter2`, i.e. `conv2(·,'same')`, kept within the patch), accumulated in
% bwlabel (column-major) order.
%
% Calls the REAL `smoothPatchesX` (addpath reference/ISI; uses the roifilt2 shim
% in tools/octave_shims). Synthetic: a smooth map + two separated patches of
% different size (so the area→σ scaling does visibly different work).
%
% Outputs (fixtures/, row-major):
%   smpatch_map.bin  (H x W f64) input map
%   smpatch_im.bin   (H x W uint8) patch image (0/1)
%   smpatch_out.bin  (H x W f64) smoothPatchesX output (oracle)
%   smpatch_meta.bin (f64) [H, W]
% Run:  via `cargo xtask goldens smoothpatches`

pkg load image;
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));
addpath(fullfile(root, 'tools', 'octave_shims'));   % roifilt2

H = 40; W = 50;
[xx, yy] = meshgrid(1:W, 1:H);
map = 20*sin(xx/5) + 15*cos(yy/6) + 0.3*xx;

im = zeros(H, W);
im(8:25, 8:22)  = 1;   % patch 1 (≈ 270 px → σ ≈ 0.13)
im(10:34, 30:46) = 1;  % patch 2 (≈ 425 px → σ ≈ 0.21)

out = smoothPatchesX(map, im);   % REAL oracle (mutates a copy of map internally)

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'smpatch_map.bin'), 'w'); fwrite(fid, map', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'smpatch_im.bin'),  'w'); fwrite(fid, uint8(im'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'smpatch_out.bin'), 'w'); fwrite(fid, out', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'smpatch_meta.bin'),'w'); fwrite(fid, [H; W], 'double'); fclose(fid);

printf('  smoothpatches: 2 patches, out range [%.3f, %.3f]\n', min(out(:)), max(out(:)));
