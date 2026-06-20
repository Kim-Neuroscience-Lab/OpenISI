% PRISTINE bridge to the GENUINE SNLC / Garrett MATLAB reference (reference/ISI),
% executed via Octave. This is the ONE place the Rust tests reach the SNLC oracle.
% It contains NO oracle algorithm: only (a) array marshalling across the process
% boundary and (b) a dispatch table mapping a function id to a DIRECT call of the
% genuine reference `.m` (addpath'd, byte-pristine). No per-golden scripts, no
% frozen fixtures — the oracle is computed live on every run.
%
% Irreducible gap (stated, not assumed away): Octave is not MATLAB. Octave's IPT
% functions match MATLAB to high precision but are not bit-identical; this is the
% SNLC oracle's analogue of NAT's period-correct-reconstruction caveat.
%
% Invoke:  octave-cli --norc -q bridge.m <request.json>
% Protocol mirrors the NAT Python bridge (raw little-endian f64 + a JSON request).

args = argv();
reqpath = args{1};
req = jsondecode(fileread(reqpath));

here = fileparts(mfilename('fullpath'));
% oracle/snlc/ -> repo root is five levels up.
repo = fullfile(here, '..', '..', '..', '..', '..');
% genpath so the genuine .m in subdirs (ISIAnGUI/F1 = Gprocesskret/adaptiveSmoother,
% ISI_Processing = shadow, …) resolve too. The reference tree stays byte-pristine;
% we only add it to Octave's search path.
addpath(genpath(fullfile(repo, 'reference', 'ISI')));   % genuine SNLC .m, pristine

% The SNLC reference uses MATLAB Image Processing Toolbox functions (bwlabel,
% imopen/imclose/imfill/imdilate, fspecial, watershed, bwdist); in Octave these
% live in the `image` package, which is part of this oracle's required env.
pkg load image;

% --- load inputs ---------------------------------------------------------------
% Rust writes row-major (C-order) f64; Octave is column-major, so read the flat
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

% --- dispatch: each arm is a DIRECT call of the genuine reference (inlined so
% Octave parses it as plain script code, no local-function lookup) -------------
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
  case 'watershed'       % raw Octave IPT builtin: watershed(A, conn={4,8})
    % The genuine oracle is Octave's own watershed; our watershed_octave{4,8}
    % wraps exactly this. conn passed as a param. Labels returned as double.
    L = watershed(x{1}, p.conn);
    outs = {double(L)};
  case 'bwdist'          % raw Octave IPT builtin: bwdist(seeds) Euclidean DT
    % Octave bwdist returns SINGLE; widening single->double here is exact, so the
    % Rust f64 side compares to f32 precision (the documented oracle dtype).
    D = bwdist(logical(x{1}));
    outs = {double(D)};
  case 'imimposemin'     % raw Octave IPT builtin: imimposemin(I, BW)
    % Morphological reconstruction imposing regional minima at BW. Genuine oracle
    % is Octave's own imimposemin; our imimposemin mirrors it.
    R = imimposemin(x{1}, logical(x{2}));
    outs = {double(R)};
  case 'imopen_disk'     % raw Octave IPT: imopen(mask, strel('disk',R,0))
    outs = {double(imopen(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imclose_disk'    % raw Octave IPT: imclose(mask, strel('disk',R,0))
    outs = {double(imclose(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imdilate_disk'   % raw Octave IPT: imdilate(mask, strel('disk',R,0))
    outs = {double(imdilate(logical(x{1}), strel('disk', p.radius, 0)))};
  case 'imfill_holes'    % raw Octave IPT: imfill(mask, 'holes')
    outs = {double(imfill(logical(x{1}), 'holes'))};
  case 'interp2_spline'  % raw Octave builtin: interp2(1:W,1:H,Z,XI,YI,'spline')
    % The genuine oracle is Octave's own not-a-knot tensor-product cubic spline.
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
printf('%s', jsonencode(struct('outputs', meta)));
