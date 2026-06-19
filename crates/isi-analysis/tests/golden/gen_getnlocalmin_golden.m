% Goldens for SNLC `getNlocalmin` (splitPatchesX subfunction) and, atomically,
% `imimposemin` (the riskiest sub-op — morphological reconstruction).
%
% getNlocalmin discretises a patch's eccentricity into percentile bins, opens +
% medians it, finds regional minima, then watersheds an imimposemin'd map to cut
% the patch into sub-regions (`newpatches`, the centerPatch2 passed to
% resetPatch). The body below is a VERBATIM inline transcription (file-local
% subfunction), calling the REAL Octave prctile/imopen/medfilt2/imregionalmin/
% imimposemin/watershed. No computational line altered.
%
% Synthetic: a horizontal-bar patch over an eccentricity field with TWO wells →
% two local minima → the patch is cut in two.
%
% Outputs (fixtures/, row-major):
%   gnlm_patch.bin / gnlm_ecc.bin      (H x W) patch mask (u8) + kmap_rad (f64)
%   gnlm_newpatches.bin                (H x W u8) newpatches (oracle)
%   gnlm_meta.bin                      (f64) [H, W, Rmax, Nmin]
%   imimpose_in.bin / _bw.bin / _out.bin  imimposemin atomic (f64 / u8 / f64)
% Run:  via `cargo xtask goldens getnlocalmin`

pkg load image;

H = 40; W = 60; Rmax = 30;
[xx, yy] = meshgrid(1:W, 1:H);
patch = zeros(H, W); patch(15:26, 10:50) = 1;
kmap_rad0 = min(sqrt((xx-20).^2 + (yy-20).^2), sqrt((xx-40).^2 + (yy-20).^2));

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'gnlm_patch.bin'), 'w'); fwrite(fid, uint8(patch'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'gnlm_ecc.bin'),   'w'); fwrite(fid, kmap_rad0', 'double'); fclose(fid);

% ── verbatim getNlocalmin (inline, on a copy) ────────────────────────────────
idpatch = find(patch);
kmap_rad = kmap_rad0;
dum = zeros(size(kmap_rad)); dum(idpatch) = 1; idnopatch = find(dum == 0);
kr = kmap_rad(idpatch);
threshdom = min(kr)-1;
for prc = 2:10:90
    threshdom = [threshdom prctile(kr,prc)];
end
threshdom = [threshdom max(kr)+1];
for i = 1:length(threshdom)-1
   id = find(kmap_rad>threshdom(i) & kmap_rad<threshdom(i+1));
   kmap_rad(id) = mean(kmap_rad(id));
end
kmap_rad(idnopatch) = max(kmap_rad(idpatch));
kmap_rad = imopen(kmap_rad, strel('disk',3,0));
rad = zeros(size(kmap_rad)); rad(idnopatch) = Rmax; rad(idpatch) = kmap_rad(idpatch);
rad = medfilt2(rad, [3 3]);
dumpatch = zeros(size(kmap_rad)); dumpatch(idpatch) = 1;
minpatch = imregionalmin(rad,8); minpatch = minpatch.*dumpatch;
D = round(sqrt(length(idpatch))/20);
minpatch = imdilate(minpatch, strel('disk',D,0)); minpatch = minpatch.*dumpatch;
Nmin = length(unique(bwlabel(minpatch,4)))-1;
dumpatch2 = imdilate(dumpatch, strel('disk',3,0));
rad2 = imimposemin(rad, minpatch);
rad2(find(1-dumpatch)) = Rmax;
rad2(find(~dumpatch2)) = -inf;
newpatches = watershed(rad2);
newpatches(find(newpatches == 1)) = 0;
newpatches(find(newpatches > 0)) = 1;
% ── end verbatim ─────────────────────────────────────────────────────────────

fid = fopen(fullfile(fixdir, 'gnlm_newpatches.bin'), 'w'); fwrite(fid, uint8(newpatches'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'gnlm_meta.bin'), 'w'); fwrite(fid, [H; W; Rmax; Nmin], 'double'); fclose(fid);
printf('  getnlocalmin: Nmin=%d, newpatches px=%d\n', Nmin, sum(newpatches(:)));

% ── imimposemin atomic ───────────────────────────────────────────────────────
B = 5 + 3*sin(xx/4) + 2*cos(yy/5);
bw = false(H, W); bw(20, 15) = 1; bw(20, 45) = 1;
Bimp = imimposemin(B, bw);
fid = fopen(fullfile(fixdir, 'imimpose_in.bin'),  'w'); fwrite(fid, B', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'imimpose_bw.bin'),  'w'); fwrite(fid, uint8(bw'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'imimpose_out.bin'), 'w'); fwrite(fid, Bimp', 'double'); fclose(fid);
printf('  imimposemin: out range [%.3f, %.3f] (min -Inf at imposed)\n', ...
       min(Bimp(isfinite(Bimp))), max(Bimp(:)));
