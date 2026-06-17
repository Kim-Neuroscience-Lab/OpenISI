% Golden for the Kalatsky-Stryker delay-subtracted cycle combine, against SNLC
% Gprocesskret.m (lines 88-99) verbatim:
%   delay = angle(exp(i*ang0) + exp(i*ang2));
%   delay = delay + pi/2*(1 - sign(delay));            % force into (0, pi]
%   kmap  = .5*(angle(exp(i*(ang0-delay))) - angle(exp(i*(ang2-delay))));
%
% ang0 (forward) varies along columns and ang2 (reverse) along rows, covering
% the full (-pi, pi) x (-pi, pi) product (so every quadrant — including the
% delay sign-flip region — is exercised), nudged off the exact ±pi boundary.
%
% Output: fixtures/combine_{ang0,ang2,kmap,delay}.bin (float64 row-major 64x64)
% Run:  octave-cli --norc gen_combine_golden.m

N = 64;
a = linspace(-pi + 0.05, pi - 0.05, N);
[A0, A2] = meshgrid(a, a);     % A0 = ang0 along cols, A2 = ang2 along rows

delay = angle(exp(1i * A0) + exp(1i * A2));
delay = delay + pi/2 * (1 - sign(delay));      % delay_hor/_vert, in (0, pi]
kmap  = 0.5 * (angle(exp(1i * (A0 - delay))) - angle(exp(1i * (A2 - delay))));

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'combine_ang0.bin'), 'w'); fwrite(fid, A0', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'combine_ang2.bin'), 'w'); fwrite(fid, A2', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'combine_kmap.bin'), 'w'); fwrite(fid, kmap', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'combine_delay.bin'), 'w'); fwrite(fid, delay', 'double'); fclose(fid);

printf('  combine: kmap range [%.4f, %.4f]  delay-flip px=%d\n', ...
       min(kmap(:)), max(kmap(:)), sum(sign(angle(exp(1i*A0)+exp(1i*A2)))(:) < 0));
