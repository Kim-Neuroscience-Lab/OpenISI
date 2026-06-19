% Golden for `interp2(x, y, Z, XI, YI, 'spline')` — the U=3 upsample splitPatchesX
% applies to the (smoothed) position maps. Octave `interp2` 'spline' dispatches
% to `__splinen__` (tensor-product 1-D cubic splines with NOT-A-KNOT end
% conditions; see spline.m). No Rust crate matches not-a-knot, so the algorithm
% is ported directly and validated at an ε-grounded tolerance.
%
% Calls the REAL Octave `interp2`. The spline is invariant to affine x/y
% rescaling, so a unit-spaced grid (1:W, 1:H) with finer linspace queries gives
% the same VALUES as splitPatchesX's mm domain — simpler + identical.
%
% Outputs (fixtures/, row-major f64):
%   i2s_z.bin     (H x W)             input grid values
%   i2s_zi.bin    (U·H x U·W)         spline upsample (oracle)
%   i2s_meta.bin  (f64) [H, W, U]
% Run:  via `cargo xtask goldens interp2spline`

H = 12; W = 15; U = 3;
[xx, yy] = meshgrid(1:W, 1:H);
Z = 10*sin(xx/3) .* cos(yy/4) + 0.5*xx + 0.3*yy;   % smooth, non-separable-ish

xi = linspace(1, W, U*W);
yi = linspace(1, H, U*H);
[XI, YI] = meshgrid(xi, yi);
ZI = interp2(1:W, 1:H, Z, XI, YI, 'spline');        % REAL Octave spline interp

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'i2s_z.bin'),  'w'); fwrite(fid, Z',  'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'i2s_zi.bin'), 'w'); fwrite(fid, ZI', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'i2s_meta.bin'),'w'); fwrite(fid, [H; W; U], 'double'); fclose(fid);

printf('  interp2spline: %dx%d -> %dx%d (U=%d), ZI[%.3f,%.3f]\n', ...
       H, W, U*H, U*W, U, min(ZI(:)), max(ZI(:)));
