% Golden for SNLC `adaptiveSmoother(gcomp, h)` — the Wiener-type adaptive filter
% applied per-direction to the complex F1 maps before cycle-combine
% (Gprocesskret.m:38-41). The low-pass kernel is SNLC's
% `L = fspecial('gaussian', 15, sigma)` (generatekret.m:75). Reproduced in
% `methods::direction_smoothing::adaptive_smoother`; validated at an ε-grounded
% tolerance (the local-variance division is f64, not bit-exact across runtimes).
%
% Calls the REAL `adaptiveSmoother.m` (addpath reference/ISI/ISIAnGUI/F1).
% Synthetic: a complex map with structured real+imag parts plus a little
% high-frequency texture, so the adaptive (variance-aware) filter does visibly
% non-uniform work.
%
% Outputs (fixtures/, row-major f64):
%   adaptsm_re_in.bin   (H x W) real(gcomp)
%   adaptsm_im_in.bin   (H x W) imag(gcomp)
%   adaptsm_re_out.bin  (H x W) real(adaptiveSmoother) (oracle)
%   adaptsm_im_out.bin  (H x W) imag(adaptiveSmoother) (oracle)
%   adaptsm_meta.bin    (f64) [H, W, sigma]
% Run:  via `cargo xtask goldens adaptsmooth`

pkg load image;
root = fullfile(fileparts(mfilename('fullpath')), '..', '..', '..', '..');
addpath(fullfile(root, 'reference', 'ISI', 'ISIAnGUI', 'F1'));   % adaptiveSmoother.m

H = 40; W = 48; sigma = 2;
[xx, yy] = meshgrid(1:W, 1:H);
re = 10*sin(xx/6) + 6*cos(yy/5) + 2*sin(xx/2).*cos(yy/2);   % structure + texture
im = 8*cos(xx/7) - 5*sin(yy/4) + 1.5*cos((xx+yy)/2);
gcomp = re + 1i*im;

h = fspecial('gaussian', 15, sigma);   % SNLC L = fspecial('gaussian',15,LP)
f = adaptiveSmoother(gcomp, h);        % REAL oracle

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'adaptsm_re_in.bin'),  'w'); fwrite(fid, real(gcomp)', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'adaptsm_im_in.bin'),  'w'); fwrite(fid, imag(gcomp)', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'adaptsm_re_out.bin'), 'w'); fwrite(fid, real(f)',     'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'adaptsm_im_out.bin'), 'w'); fwrite(fid, imag(f)',     'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'adaptsm_meta.bin'),   'w'); fwrite(fid, [H; W; sigma], 'double'); fclose(fid);

printf('  adaptsmooth: %dx%d sigma=%d, re[%.2f,%.2f] im[%.2f,%.2f]\n', ...
       H, W, sigma, min(real(f)(:)), max(real(f)(:)), min(imag(f)(:)), max(imag(f)(:)));
