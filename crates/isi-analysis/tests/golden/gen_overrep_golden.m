% Golden for the SNLC/Garrett visual-space COVERAGE primitive `overRep` — the
% bedrock of both splitPatchesX and fusePatchesX (the Garrett-2014 refinement
% lineage), the SNLC analog of Allen's `getVisualSpace`.
%
% `overRep` projects a cortical patch into a sphere-domain (visual-field) grid
% and returns the unique covered area (`ActualCoverage`, deg^2) plus the
% Jacobian/sigma coverage (`JacCoverage`, deg^2). The split criterion keys on the
% redundancy between them.
%
% The marked block below is a VERBATIM transcription of the subfunction in
%   reference/ISI/splitPatchesX.m : 188-215  (identical copy in fusePatchesX.m)
% inlined (it is a file-local subfunction, not callable directly — copied here
% per the same discipline as gen_combine_golden.m). No computational line is
% altered; only `pixpermm`/`Jac`/`patch` are run on copies so the input fixtures
% keep their original values.
%
% Synthetic input: a cortical patch whose azimuth map is FOLDED (tent), so the
% patch covers part of visual space redundantly — ActualCoverage collapses the
% fold while JacCoverage counts the full cortical extent (the split signal).
%
% Outputs (fixtures/, row-major):
%   overrep_kmaph.bin / overrep_kmapv.bin   (H x W f64) position maps (deg)
%   overrep_patch.bin                       (H x W f64) patch mask 0/1
%   overrep_jac.bin                         (H x W f64) Jacobian det (input Jac)
%   overrep_spcov.bin                       (Nsph x Nsph f64) coverage grid (oracle)
%   overrep_meta.json                       dims + scalars
% Run:  via `cargo xtask goldens overrep`

pkg load image;

H = 60; W = 60; pixpermm = 39; U = 1;

% Monotonic maps → a single, consistent field sign (one visual area), which is
% overRep's domain (it restricts to the dominant-sign pixels). A same-sign
% *redundant* fold is exercised later via the split orchestration, not here.
[xx, yy] = meshgrid(1:W, 1:H);
xc = (W + 1) / 2; yc = (H + 1) / 2;
kmap_hor  = 40 * (xx - xc) / (W/2);           % monotonic azimuth ~[-40,40] deg
kmap_vert = 40 * (yy - yc) / (H/2);           % monotonic altitude ~[-40,40] deg

patch = zeros(H, W);
patch(11:50, 11:50) = 1;

[dhdx, dhdy] = gradient(kmap_hor);
[dvdx, dvdy] = gradient(kmap_vert);
Jac = (dhdx .* dvdy - dvdx .* dhdy) * pixpermm^2;

sphdom = -90:90;
[sphX, sphY] = meshgrid(sphdom, sphdom);

% Write input fixtures BEFORE overRep (which mutates Jac/patch on its copies).
fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'overrep_kmaph.bin'), 'w'); fwrite(fid, kmap_hor', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'overrep_kmapv.bin'), 'w'); fwrite(fid, kmap_vert', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'overrep_patch.bin'), 'w'); fwrite(fid, patch', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'overrep_jac.bin'), 'w'); fwrite(fid, Jac', 'double'); fclose(fid);

% ── verbatim overRep (splitPatchesX.m:188-215), inlined on copies ─────────────
ppm = pixpermm*U;
N = length(sphdom);
Jacx = Jac; patchx = patch;
posneg = sign(mean(Jacx(find(patchx))));
id = find(sign(Jacx)~=posneg | Jacx == 0);
Jacx(id) = 0;
patchx(id) = 0;
idpatch = find(patchx);
JacCoverage = abs(sum(abs(Jacx(idpatch))))/ppm^2; %deg^2
sphlocX = round(kmap_hor(idpatch));
sphlocX = sphlocX-sphdom(1)+1;
sphlocY = round(kmap_vert(idpatch));
sphlocY = sphlocY-sphdom(1)+1;
sphlocVec = N*(sphlocX-1) + sphlocY;
spCov = zeros(size(sphX));
spCov(sphlocVec) = 1;
spCov = imfill(spCov);
SE = strel('disk',1,0);
spCov = imclose(spCov,SE);
spCov = imfill(spCov);
ActualCoverage = sum(spCov(:)); %deg^2
MagFac = ActualCoverage/length(idpatch);
% ── end verbatim ─────────────────────────────────────────────────────────────

% Mask → uint8 (the project's mask-fixture convention, for test_support::count_differing).
fid = fopen(fullfile(fixdir, 'overrep_spcov.bin'), 'w'); fwrite(fid, uint8(spCov'), 'uint8'); fclose(fid);

% Binary meta (f64 vector), matching the patchvs_meta convention:
%   [H, W, Nsph, sphmin, pixpermm, U, JacCoverage, ActualCoverage, MagFac]
metavec = [H; W; length(sphdom); sphdom(1); pixpermm; U; JacCoverage; ActualCoverage; MagFac];
fid = fopen(fullfile(fixdir, 'overrep_meta.bin'), 'w'); fwrite(fid, metavec, 'double'); fclose(fid);

printf('  overrep: patch=%dpx JacCov=%.1f ActualCov=%.1f MagFac=%.3f spcov=%dpx (%dx%d)\n', ...
       sum(patch(:)), JacCoverage, ActualCoverage, MagFac, sum(spCov(:)), N, N);
