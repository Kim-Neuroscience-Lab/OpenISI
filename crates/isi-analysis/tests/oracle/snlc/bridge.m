% PRISTINE bridge to the GENUINE SNLC / Garrett MATLAB reference (reference/ISI),
% executed under genuine MATLAB. This is the ONE place the Rust tests reach the SNLC
% oracle. SNLC is MATLAB code, so genuine MATLAB is the authoritative reference.
% It contains NO oracle algorithm: only (a) array marshalling across the process
% boundary and (b) a dispatch table mapping a function id to a DIRECT call of the
% genuine reference `.m` (addpath'd, byte-pristine). No per-golden scripts, no
% frozen fixtures — the oracle is computed live on every run.
%
% Invoke:  matlab -batch bridge   (with OPENISI_ORACLE_REQ pointing at request.json)
% Protocol mirrors the NAT Python bridge (raw little-endian f64 + a JSON request).

reqpath = getenv('OPENISI_ORACLE_REQ');
req = jsondecode(fileread(reqpath));

here = fileparts(mfilename('fullpath'));
% oracle/snlc/ -> repo root is five levels up.
repo = fullfile(here, '..', '..', '..', '..', '..');
% genpath so the genuine .m in subdirs (ISIAnGUI/F1 = Gprocesskret/adaptiveSmoother,
% ISI_Processing = shadow, …) resolve too. The reference tree stays byte-pristine;
% we only add it to the search path.
addpath(genpath(fullfile(repo, 'reference', 'ISI')));   % genuine SNLC .m, pristine
% The SNLC reference uses Image Processing Toolbox functions (bwlabel, imopen/imclose/
% imfill/imdilate, fspecial, watershed, bwdist, roifilt2) — all built into MATLAB.

% --- load inputs ---------------------------------------------------------------
% Rust writes row-major (C-order) f64; MATLAB is column-major, so read the flat
% buffer into [W H] and transpose to recover the HxW matrix exactly.
x = {};
for i = 1:numel(req.inputs)
  s = req.inputs(i);
  fid = fopen(s.path, 'r');
  buf = fread(fid, Inf, 'double');
  fclose(fid);
  H = s.shape(1); W = s.shape(2);
  x{i} = reshape(buf, [W, H])';
end

% --- dispatch: each arm is a DIRECT call of the genuine reference (inlined as
% plain script code, no local-function lookup) ---------------------------------
p = req.params;
switch req.fn
  case 'identity'        % de-risk: marshalling must round-trip exactly
    outs = {x{1}};
  case 'getPatchCoM'     % genuine SNLC getPatchCoM(imseg) -> [CoMxy, Axisxy]
    [CoMxy, Axisxy] = getPatchCoM(x{1});
    outs = {CoMxy, Axisxy};
  case 'getPatchSign'    % genuine SNLC getPatchSign(im, imsign) -> [patchSign, _]
    % patchSign is a per-pixel map: each patch's pixels = sign(mean)+1.1 (so
    % -1->0.1, 0->1.1, +1->2.1), background 0. Label-INVARIANT (no bwlabel-order
    % dependency), which is what we compare against.
    [patchSign, ~] = getPatchSign(x{1}, x{2});
    outs = {patchSign};
  case 'watershed'       % raw IPT builtin: watershed(A, conn={4,8})
    % The oracle is the genuine watershed; our watershed_meyer{4,8}
    % wraps exactly this. conn passed as a param. Labels returned as double.
    L = watershed(x{1}, p.conn);
    outs = {double(L)};
  case 'bwdist'          % raw IPT builtin: bwdist(seeds) Euclidean DT
    % MATLAB bwdist returns SINGLE; widening single->double here is exact, so the
    % Rust f64 side compares to f32 precision (the documented oracle dtype).
    D = bwdist(logical(x{1}));
    outs = {double(D)};
  case 'imimposemin'     % raw IPT builtin: imimposemin(I, BW)
    % Morphological reconstruction imposing regional minima at BW. Genuine oracle
    % is the genuine imimposemin; our imimposemin mirrors it.
    R = imimposemin(x{1}, logical(x{2}));
    outs = {double(R)};
  case 'imopen_disk'     % raw IPT: imopen(mask, strel('disk',R,0))
    outs = {double(imopen(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imclose_disk'    % raw IPT: imclose(mask, strel('disk',R,0))
    outs = {double(imclose(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imdilate_disk'   % raw IPT: imdilate(mask, strel('disk',R,0))
    outs = {double(imdilate(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imfill_holes'    % raw IPT: imfill(mask, 'holes')
    outs = {double(imfill(logical(x{1}), 'holes'))};
  case 'fft_gaussian'    % fft-based circular Gaussian blur (the SNLC smoother)
    % mapS = real(ifft2(fft2(map) .* abs(fft2(fspecial('gaussian',size,sigma))))).
    % the reference's fspecial/fft are the oracle; the bridge only calls them.
    hh = fspecial('gaussian', size(x{1}), p.sigma);
    outs = {real(ifft2(fft2(x{1}) .* abs(fft2(hh))))};
  case 'adaptive_smoother'  % genuine SNLC adaptiveSmoother.m (Wiener-type adaptive filter)
    % x{1}=real, x{2}=imag of the complex F1 map. h = fspecial('gaussian',15,sigma)
    % (SNLC L = fspecial('gaussian',15,LP), generatekret.m:75). Genuine .m is the
    % oracle (addpath'd via genpath: reference/ISI/ISIAnGUI/F1). Returns re/im.
    gcomp = x{1} + 1i * x{2};
    h = fspecial('gaussian', 15, p.sigma);
    f = adaptiveSmoother(gcomp, h);
    outs = {real(f), imag(f)};
  case 'interp2_spline'  % raw builtin: interp2(1:W,1:H,Z,XI,YI,'spline')
    % The oracle is the genuine not-a-knot tensor-product cubic spline.
    % x{1}=Z (HxW), x{2}=xi (1 x U*W), x{3}=yi (1 x U*H). Unit-spaced source grid
    % (the spline is affine-invariant in x/y, as splitPatchesX relies on).
    Z = x{1};
    [Hh, Ww] = size(Z);
    xi = x{2}(:)'; yi = x{3}(:)';
    [XI, YI] = meshgrid(xi, yi);
    outs = {interp2(1:Ww, 1:Hh, Z, XI, YI, 'spline')};
  case 'gprocesskret_hor'  % genuine SNLC Gprocesskret.m (Kalatsky combine + delay)
    % Inputs are the fwd/rev PHASE maps (radians) AFTER Gprocesskret's internal
    % negation (i.e. what the Rust Complex2::from_phase consumes). Gprocesskret's
    % no-smoothing branch does ang = angle(-ang_input), so to make its internal
    % angle equal x{i} we feed ang_input = -exp(i*x{i}); then angle(-ang_input) =
    % x{i} and its kmap_hor/delay_hor match our function fed exp(i*x{i}). No
    % smoothing (hl=hh=[], adaptbit=false); vertical slots are dummies. Returns
    % kmap_hor and delay_hor in DEGREES (bw=1 everywhere).
    ang0 = -exp(1i * x{1});
    ang2 = -exp(1i * x{2});
    bw = ones(size(ang0));
    [kmap_hor, kmap_vert, delay_hor, delay_vert, sh, magS] = ...
        Gprocesskret({ang0, ang0, ang2, ang2}, bw, false, [], []);
    outs = {kmap_hor, delay_hor};
  case 'gprocesskret_mags'  % genuine Gprocesskret magS.hor = (|ang0|+|ang2|)/2
    % Full complex fwd/rev (re/im); magS is taken from the input magnitudes
    % BEFORE the negation, so no transform is needed. x{1..4}=fwd_re,fwd_im,
    % rev_re,rev_im. No smoothing branch.
    ang0 = x{1} + 1i * x{2};
    ang2 = x{3} + 1i * x{4};
    bw = ones(size(ang0));
    [k1, k2, d1, d2, sh, magS] = Gprocesskret({ang0, ang0, ang2, ang2}, bw, false, [], []);
    outs = {magS.hor};
  case 'splitpatchesx'   % genuine SNLC splitPatchesX(im,kmap_hor,kmap_vert,kmap_rad,pixpermm)
    % The over-representation split. figTag=0 (no plotting). smoothPatchesX uses
    % roifilt2 (MATLAB Image Processing Toolbox). x{1}=im (binary patch map),
    % x{2}=kmap_hor (azimuth deg), x{3}=kmap_vert (altitude deg), x{4}=kmap_rad
    % (eccentricity deg). Returns the refined binary patch map.
    outs = {double(splitPatchesX(logical(x{1}), x{2}, x{3}, x{4}, p.pixpermm))};
  case 'fusepatchesx'    % genuine SNLC fusePatchesX(im,kmap_hor,kmap_vert,pixpermm)
    % Fuse pairs of patches whose visual-space coverage overlaps. MATLAB-only (its
    % overRep subfunction path mirrors splitPatchesX's; figTag=0 → headless).
    [imf, ~] = fusePatchesX(logical(x{1}), x{2}, x{3}, p.pixpermm);
    outs = {double(imf)};
  otherwise
    error('unknown oracle fn %s', req.fn);
end

% --- write outputs (row-major: fwrite of the transpose) ------------------------
meta = struct('file', {}, 'dtype', {}, 'shape', {});
for i = 1:numel(outs)
  a = double(outs{i});
  [oh, ow] = size(a);
  fpath = fullfile(req.out_dir, sprintf('out%d.bin', i - 1));
  fid = fopen(fpath, 'w');
  fwrite(fid, a', 'double');
  fclose(fid);
  meta(i) = struct('file', fpath, 'dtype', '<f8', 'shape', [oh, ow]);
end
resp = jsonencode(struct('outputs', meta));
% The harness reads response.json (a file) — `matlab -batch` stdout can carry
% startup noise, so we never rely on stdout for the payload.
rfid = fopen(fullfile(req.out_dir, 'response.json'), 'w');
fprintf(rfid, '%s', resp);
fclose(rfid);
