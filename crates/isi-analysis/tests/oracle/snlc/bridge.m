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
addpath(fullfile(repo, 'reference', 'ISI'));   % genuine SNLC .m, pristine

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
