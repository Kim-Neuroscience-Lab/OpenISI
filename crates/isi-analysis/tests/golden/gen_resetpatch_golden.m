% Golden for SNLC `resetPatch` (subfunction in splitPatchesX.m) — the watershed
% SPLIT: when limiting a patch to the central visual field fragments it into ≥2
% connected components, cut the original patch into sub-patches via a
% distance-transform watershed seeded at those components.
%
% The marked block is a VERBATIM transcription of the `resetPatch` subfunction
% (file-local, copied like overRep), calling the REAL Octave `bwdist`,
% `watershed`, `imdilate/erode/open`, `bwlabel`, and the real `phi.m` (addpath).
% No computational line altered.
%
% Synthetic: ONE patch shaped as a dumbbell (two blobs + a 1-px neck). The neck
% survives in the patch but is removed by the disk-1 opening of the center
% region → 2 components → the split fires, cutting the dumbbell in two.
%
% Outputs (fixtures/, row-major):
%   resetpatch_im.bin       (H x W uint8) input patch image (the dumbbell)
%   resetpatch_center.bin   (H x W uint8) centerPatch mask
%   resetpatch_out.bin      (H x W uint8) resetPatch output (split sub-patches)
%   resetpatch_meta.bin     (f64) [H, W, q, n_subpatches_out]
% Run:  via `cargo xtask goldens resetpatch`

pkg load image;
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));   % the real `phi.m`

H = 50; W = 60;
im = zeros(H, W);
im(15:34, 8:24)  = 1;    % left blob
im(15:34, 36:52) = 1;    % right blob
im(24:25, 25:35) = 1;    % 2-px neck joining them (one dumbbell)
centerPatch = im;        % whole patch is within the central field

imlab = bwlabel(im, 4);
q = 1;                   % the single dumbbell patch

% Write inputs before the (mutating) transcription.
fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'resetpatch_im.bin'), 'w');     fwrite(fid, uint8(im'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'resetpatch_center.bin'), 'w'); fwrite(fid, uint8(centerPatch'), 'uint8'); fclose(fid);

% ── verbatim resetPatch (splitPatchesX.m subfunction) ─────────────────────────
idorig = find(imlab == q);
imorigpatch = zeros(size(im));
imorigpatch(idorig) = 1;
SE = strel('disk',1,0);
imdilpatch = imdilate(imorigpatch,SE);
idpatch = find(imlab == q & centerPatch);
impatch = zeros(size(im));
impatch(idpatch) = 1;
SE = strel('disk',1,0);
impatch = imopen(impatch,SE);
idpatch = find(impatch);
imlabdum = bwlabel(impatch,4);
idlab  = unique(imlabdum);
if length(idlab) > 2
    imdist = bwdist(impatch);
    id = find(~imdilpatch);
    imdist(id) = -inf;
    imdist(idpatch) = 0;
    wshed = watershed(imdist,4);
    wshed = sign(phi(wshed-1));
    SE = strel('disk',1,0);
    wshed = imerode(wshed,SE);
    im(idorig) = 0;
    im = im+wshed;
end
% ── end verbatim ─────────────────────────────────────────────────────────────

n_out = max(max(bwlabel(im, 4)));
fid = fopen(fullfile(fixdir, 'resetpatch_out.bin'), 'w'); fwrite(fid, uint8(im'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'resetpatch_meta.bin'), 'w'); fwrite(fid, [H; W; q; n_out], 'double'); fclose(fid);

printf('  resetpatch: split into %d sub-patches, out=%dpx\n', n_out, sum(im(:) > 0));
