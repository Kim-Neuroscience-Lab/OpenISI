% Figure-data generator (NOT a committed golden — note the `figdata_` prefix, so
% `xtask goldens` skips it): run the **real** SNLC/Garrett segmentation oracle on
% R43's actual retinotopy, for the oracle-state gallery's r43_oracle figure.
%
% Inputs (written by `cargo run -p isi-analysis --example oracle_state -- r43`):
%   target/oracle_state/r43/oracle_in/{kmap_hor,kmap_vert}.bin  (azim/alt deg, f64)
%   target/oracle_state/r43/oracle_in/meta.json                 ({h,w,pixpermm})
%
% This transcribes the *orchestration* of `reference/ISI/getMouseAreasX.m`
% (verbatim, minus the figure/plot calls) but calls the REAL SNLC functions on
% the MATLAB path — crucially `splitPatchesX` / `fusePatchesX` (the patch
% refinement OpenISI never ported to Rust), plus `getPatchSign` / `getPatchCoM`
% / `getV1id`. So column 1 of the r43_oracle figure is genuine oracle output,
% not OpenISI's own baseline.
%
% Outputs: target/oracle_state/r43_oracle/{snlc_vfs,snlc_areas}.oracle.bin
% Run via genuine MATLAB:  matlab -batch figdata_oracle_state_snlc

here = fileparts(mfilename('fullpath'));
root = fullfile(here, '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI'));            % the SNLC oracle source
% IPT (imopen/imclose/imfill/imdilate/bwlabel/bwmorph) and roifilt2 (used by
% splitPatchesX/fusePatchesX) are built into MATLAB — no packages, no shims.

ind  = fullfile(root, 'target', 'oracle_state', 'r43', 'oracle_in');
outd = fullfile(root, 'target', 'oracle_state', 'r43_oracle');
if ~exist(outd, 'dir'); mkdir(outd); end

meta = jsondecode(fileread(fullfile(ind, 'meta.json')));
H = meta.h; W = meta.w; pixpermm = meta.pixpermm;

% Row-major f64 on disk → H×W MATLAB matrix (reshape is column-major, so transpose).
fid = fopen(fullfile(ind, 'kmap_hor.bin'),  'r'); kmap_hor  = reshape(fread(fid, H*W, 'double'), W, H)'; fclose(fid);
fid = fopen(fullfile(ind, 'kmap_vert.bin'), 'r'); kmap_vert = reshape(fread(fid, H*W, 'double'), W, H)'; fclose(fid);

% ── getMouseAreasX.m orchestration (verbatim compute; plotting removed) ──────

[dhdx, dhdy] = gradient(kmap_hor);
[dvdx, dvdy] = gradient(kmap_vert);
graddir_hor  = atan2(dhdy, dhdx);
graddir_vert = atan2(dvdy, dvdx);
vdiff = exp(1i*graddir_hor) .* exp(-1i*graddir_vert);
VFS = sin(angle(vdiff));
VFS(isnan(VFS)) = 0;
hh = fspecial('gaussian', size(VFS), 3); hh = hh/sum(hh(:));
VFS = ifft2(fft2(VFS) .* abs(fft2(hh)));         % smooth before thresholding

gradmag  = abs(VFS);
threshSeg = 1.5*std(VFS(:));
imseg = (sign(gradmag - threshSeg/2) + 1)/2;     % threshold at +/-1.5 sigma

SE = strel('disk', 2, 0);
imseg = imopen(imseg, SE);

% Boundary of visual cortex (pad → imclose/imfill/imdilate → unpad).
Npad = 30; dim = size(imseg);
imsegpad = [zeros(dim(1),Npad) imseg zeros(dim(1),Npad)];
dim = size(imsegpad);
imsegpad = [zeros(Npad,dim(2)); imsegpad; zeros(Npad,dim(2))];
imbound = imclose(imsegpad, strel('disk',10,0));
imbound = imfill(imbound);
imbound = imdilate(imbound, strel('disk',3,0));
imbound = imfill(imbound);
imbound = imbound(Npad+1:end-Npad, Npad+1:end-Npad);
imbound(:,1) = 0; imbound(:,end) = 0; imbound(1,:) = 0; imbound(end,:) = 0;

% Keep only the main connected group of patches.
bwlab = bwlabel(imbound, 4);
labid = unique(bwlab);
S = zeros(1, length(labid));
for i = 1:length(labid); S(i) = sum(bwlab(:) == labid(i)); end
S(1) = 0;                                         % ignore the surround
[~, mi] = max(S);
imbound = double(bwlab == labid(mi));
imseg = imseg .* imbound;
imseg(:,1:2) = 0; imseg(:,end-1:end) = 0; imseg(1:2,:) = 0; imseg(end-1:end,:) = 0;

% Thinning → one-pixel borders → patch image.
bordr = imbound - imseg;
bordr = bwmorph(bordr, 'thin', Inf);
bordr = bwmorph(bordr, 'spur', 4);
im = bwlabel(1 - bordr, 4); im(im == 1) = 0; im = sign(im);

% Eccentricity about V1's center-of-mass (needed by the split criterion).
imdum = imopen(imseg, strel('disk',10,0));
CoMxy = getPatchCoM(imdum);
V1id  = getV1id(imdum);
Vcent1 = kmap_hor (round(CoMxy(V1id,2)), round(CoMxy(V1id,1)));
Vcent2 = kmap_vert(round(CoMxy(V1id,2)), round(CoMxy(V1id,1)));
az  = (kmap_hor  - Vcent1)*pi/180;
alt = (kmap_vert - Vcent2)*pi/180;
kmap_rad = atan(sqrt(tan(az).^2 + (tan(alt).^2)./(cos(az).^2)))*180/pi;

% THE un-ported oracle steps: split redundant patches, fuse exclusive ones.
im = splitPatchesX(im, kmap_hor, kmap_vert, kmap_rad, pixpermm);
bordr = imbound - im;
bordr = bwmorph(bordr, 'thin', Inf);
bordr = bwmorph(bordr, 'spur', 4);
im = bwlabel(1 - bordr, 4); im(im == 1) = 0; im = sign(im);
im = imopen(im, strel('disk',2,0));
[im, ~] = fusePatchesX(im, kmap_hor, kmap_vert, pixpermm);

patchmask = double(im > 0);

% ── outputs (row-major on disk: transpose so column-major fwrite emits row-major) ─
fid = fopen(fullfile(outd, 'snlc_vfs.oracle.bin'),   'w'); fwrite(fid, VFS',       'double'); fclose(fid);
fid = fopen(fullfile(outd, 'snlc_areas.oracle.bin'), 'w'); fwrite(fid, patchmask', 'double'); fclose(fid);
fprintf('SNLC oracle on R43: %d px in patches, VFS range [%.3f %.3f]\n', ...
        sum(patchmask(:)), min(VFS(:)), max(VFS(:)));
