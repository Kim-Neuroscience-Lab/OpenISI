% Golden for SNLC `fusePatchesX` (the under-segmentation merge), end-to-end.
% Calls the REAL `fusePatchesX` (addpath reference/ISI; uses the roifilt2 shim).
% Its fuse branch PLOTS unconditionally (not figTag-guarded), so figures are set
% invisible for headless generation.
%
% Synthetic: a monotonic retinotopy (one field sign) with two patches that are
% adjacent (2-px gap → touch after disk-3 dilation) and cover DISJOINT azimuth
% ranges (visual overlap ≈ 0 < 10%) → the pair is fused into one patch.
%
% Outputs (fixtures/, row-major):
%   fpx_im.bin / fpx_hor.bin / fpx_vert.bin   inputs (u8 / f64×2)
%   fpx_out.bin                               fusePatchesX output (u8)
%   fpx_meta.bin   (f64) [H, W, pixpermm]
% Run:  via `cargo xtask goldens fusepatchesx`

pkg load image;
set(0, 'defaultfigurevisible', 'off');   % headless; fuse branch plots
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));
addpath(fullfile(root, 'tools', 'octave_shims'));

H = 40; W = 60; pixpermm = 39;
[xx, yy] = meshgrid(1:W, 1:H);
kmap_hor  = 1.5 * (xx - 30);     % azimuth ramp (one sign)
kmap_vert = 1.5 * (yy - 20);     % altitude ramp

im = zeros(H, W);
im(12:28, 15:27) = 1;            % patch 1 (azimuth ≈ [-22, -5])
im(12:28, 30:42) = 1;            % patch 2 (azimuth ≈ [0, 18]); 2-px gap

out = fusePatchesX(im, kmap_hor, kmap_vert, pixpermm);   % REAL oracle

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'fpx_im.bin'),   'w'); fwrite(fid, uint8(im'),  'uint8');  fclose(fid);
fid = fopen(fullfile(fixdir, 'fpx_hor.bin'),  'w'); fwrite(fid, kmap_hor',   'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'fpx_vert.bin'), 'w'); fwrite(fid, kmap_vert',  'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'fpx_out.bin'),  'w'); fwrite(fid, uint8(out'), 'uint8');  fclose(fid);
fid = fopen(fullfile(fixdir, 'fpx_meta.bin'), 'w'); fwrite(fid, [H; W; pixpermm], 'double'); fclose(fid);

printf('  fusepatchesx: in patches=%d -> out patches=%d (out px=%d)\n', ...
       max(max(bwlabel(im,4))), max(max(bwlabel(out,4))), sum(out(:)));
