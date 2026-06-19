% Golden for the assembled SNLC `splitPatchesX` (the over-representation split,
% end-to-end). Calls the REAL `splitPatchesX` (addpath reference/ISI; figTag=0,
% so no plotting; uses the roifilt2 shim) — which internally drives smoothPatchesX,
% the fft-Gaussian, interp2('spline'), getCenterPatch, resetPatch, overRep, and
% getNlocalmin (all of which are individually golden-validated).
%
% Synthetic: a clean monotonic retinotopy (eccentricity < 30° within the
% patches) with two well-separated patches. The center-limit pass is a no-op,
% the minima pass runs getNlocalmin but finds one well per patch (no split), and
% the coverage pass keeps both (CovOverlap ≈ 1, ample coverage) — so the loops
% are exercised while the decisions stay clear of their thresholds, and the
% output is the open→erode of the input. (A split-triggering case is covered
% atomically by the reset_patch / get_nlocalmin goldens.)
%
% Outputs (fixtures/, row-major):
%   spx_im.bin / spx_hor.bin / spx_vert.bin / spx_rad.bin   inputs (u8 / f64×3)
%   spx_out.bin                                             splitPatchesX output (u8)
%   spx_meta.bin   (f64) [H, W, pixpermm]
% Run:  via `cargo xtask goldens splitpatchesx`

pkg load image;
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));
addpath(fullfile(root, 'tools', 'octave_shims'));   % roifilt2

H = 40; W = 60; pixpermm = 39;
[xx, yy] = meshgrid(1:W, 1:H);
kmap_hor  = 0.8 * (xx - 30);     % azimuth (deg), monotonic
kmap_vert = 0.8 * (yy - 20);     % altitude (deg), monotonic
kmap_rad  = sqrt(kmap_hor.^2 + kmap_vert.^2);   % eccentricity (< 30° in patches)

im = zeros(H, W);
im(10:25, 10:25)  = 1;
im(12:28, 38:52)  = 1;

out = splitPatchesX(im, kmap_hor, kmap_vert, kmap_rad, pixpermm);   % REAL oracle

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'spx_im.bin'),   'w'); fwrite(fid, uint8(im'),  'uint8');  fclose(fid);
fid = fopen(fullfile(fixdir, 'spx_hor.bin'),  'w'); fwrite(fid, kmap_hor',   'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'spx_vert.bin'), 'w'); fwrite(fid, kmap_vert',  'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'spx_rad.bin'),  'w'); fwrite(fid, kmap_rad',   'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'spx_out.bin'),  'w'); fwrite(fid, uint8(out'), 'uint8');  fclose(fid);
fid = fopen(fullfile(fixdir, 'spx_meta.bin'), 'w'); fwrite(fid, [H; W; pixpermm], 'double'); fclose(fid);

printf('  splitpatchesx: in=%dpx -> out=%dpx, patches=%d\n', ...
       sum(im(:)), sum(out(:)), max(max(bwlabel(out, 4))));
