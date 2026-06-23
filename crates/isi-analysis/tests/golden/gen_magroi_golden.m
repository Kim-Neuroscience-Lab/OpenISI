% Golden for the SNLC response-magnitude ROI gate — verbatim overlaymaps.m:205-215:
%   mag = magf.^1.1;          % raise to the exponent
%   mag = mag - min(mag(:));  % normalize to [0,1] over the whole frame
%   mag = mag / max(mag(:));
%   magROI = mag >= thresh;   % keep pixels at/above the threshold (here .12)
% A pure intensity gate (no morphology). Ported in
% `methods::cortex_source::snlc_mag_threshold_roi` and validated bit-for-bit
% (boolean mask ⇒ exact, no tolerance).
%
% Synthetic magf: two Gaussian bumps + a faint ramp, all positive, so the
% normalized threshold carves a non-trivial multi-region ROI.
%
% Outputs (fixtures/):
%   magroi_in.npy   (H x W) f64 input magnitude `magf`
%   magroi_out.npy  (H x W) uint8 boolean ROI mask (oracle)
%   magroi_meta.npy (f64) [H, W, exponent, thresh]
% Run:  matlab -batch gen_magroi_golden   (via `cargo xtask goldens magroi`)

H = 40; W = 48; exponent = 1.1; thresh = 0.12;
[xx, yy] = meshgrid(1:W, 1:H);
magf = 1.0 ...
     + 8*exp(-((xx-14).^2 + (yy-13).^2)/40) ...
     + 5*exp(-((xx-35).^2 + (yy-28).^2)/60) ...
     + 0.5*xx/W;

mag = magf .^ exponent;
mag = mag - min(mag(:));
mag = mag / max(mag(:));
magROI = double(mag >= thresh);

scriptdir = fileparts(mfilename('fullpath'));
addpath(scriptdir);  % for save_npy
fixdir = fullfile(scriptdir, 'fixtures');
save_npy(fullfile(fixdir, 'magroi_in.npy'),   magf,                       '<f8');
save_npy(fullfile(fixdir, 'magroi_out.npy'),  uint8(magROI),              '|u1');
save_npy(fullfile(fixdir, 'magroi_meta.npy'), [H; W; exponent; thresh],   '<f8');

fprintf('  magroi: %dx%d exp=%.2f thr=%.2f -> %d/%d px in ROI\n', ...
        H, W, exponent, thresh, sum(magROI(:)), H*W);
