% Golden for SNLC `getCenterPatch` (subfunction in splitPatchesX.m) — the patch
% region within R degrees of the visual-field center, cleaned up. splitPatchesX
% uses it to decide whether limiting a patch to the central visual field
% fragments it (→ a redundant patch that should be split).
%
% The marked block is a VERBATIM transcription of the `getCenterPatch`
% subfunction in reference/ISI/splitPatchesX.m (it is file-local, so copied here
% like overRep / per gen_combine_golden.m discipline). The real `imopen`
% (strel disk 2) and `medfilt2` (3×3 median) run under Octave; no line altered.
%
% Synthetic: a radial eccentricity field centered in-frame, R=30°, and two
% patches — one straddling the center, one mostly outside R — so the threshold,
% the disk-2 opening, and the 3×3 median all do visible work.
%
% Outputs (fixtures/, row-major):
%   centerpatch_im.bin    (H x W uint8) patch image (0/1)
%   centerpatch_ecc.bin   (H x W f64)   eccentricity kmap_rad (deg)
%   centerpatch_out.bin   (H x W uint8) getCenterPatch output mask (oracle)
%   centerpatch_meta.bin  (f64) [H, W, R]
% Run:  via `cargo xtask goldens centerpatch`

pkg load image;

H = 60; W = 60; R = 30;
[xx, yy] = meshgrid(1:W, 1:H);
cx = 30; cy = 30;
kmap_rad = sqrt((xx - cx).^2 + (yy - cy).^2);   % eccentricity (deg), 0..~41

im = zeros(H, W);
im(10:50, 10:30) = 1;    % patch straddling the center
im(15:45, 40:55) = 1;    % patch mostly outside R

% Write inputs before the (potentially mutating) transcription.
fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'centerpatch_im.bin'), 'w');  fwrite(fid, uint8(im'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'centerpatch_ecc.bin'), 'w'); fwrite(fid, kmap_rad', 'double'); fclose(fid);

% ── verbatim getCenterPatch (splitPatchesX.m subfunction) ─────────────────────
id = find(kmap_rad < R);
centerPatch = zeros(size(im));
centerPatch(id) = 1;
centerPatch = centerPatch .* im;
SE = strel('disk', 2, 0);
centerPatch = imopen(centerPatch, SE);
centerPatch = medfilt2(centerPatch, [3 3]);
% ── end verbatim ─────────────────────────────────────────────────────────────

fid = fopen(fullfile(fixdir, 'centerpatch_out.bin'), 'w'); fwrite(fid, uint8(centerPatch'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'centerpatch_meta.bin'), 'w'); fwrite(fid, [H; W; R], 'double'); fclose(fid);

printf('  centerpatch: in=%dpx -> out=%dpx (R=%d)\n', sum(im(:)), sum(centerPatch(:)), R);
