% Golden for the fft-based circular Gaussian smooth used in splitPatchesX setup:
%   hh   = fspecial('gaussian', size(map), sigma);
%   mapS = real(ifft2( fft2(map) .* abs(fft2(hh)) ));
% i.e. a zero-phase circular Gaussian blur (the `abs` strips the centering
% phase of fspecial's centered kernel). Ported with an FFT crate (rustfft) and
% validated at an ε-grounded tolerance (cross-library FFT roundoff ⇒ not bit-exact).
%
% Calls the REAL Octave ops. Synthetic: a couple of Gaussian bumps + a ramp, so
% the smoothing is visible and non-trivial.
%
% Outputs (fixtures/, row-major f64):
%   fftgauss_in.bin   (H x W) input map
%   fftgauss_out.bin  (H x W) smoothed map (oracle)
%   fftgauss_meta.bin (f64) [H, W, sigma]
% Run:  via `cargo xtask goldens fftgauss`

pkg load image;

H = 40; W = 48; sigma = 2;
[xx, yy] = meshgrid(1:W, 1:H);
map = 30*exp(-((xx-15).^2 + (yy-12).^2)/50) ...
    + 20*exp(-((xx-34).^2 + (yy-30).^2)/80) ...
    + 5*xx/W;

hh   = fspecial('gaussian', size(map), sigma);
mapS = real(ifft2(fft2(map) .* abs(fft2(hh))));

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'fftgauss_in.bin'),  'w'); fwrite(fid, map',  'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'fftgauss_out.bin'), 'w'); fwrite(fid, mapS', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'fftgauss_meta.bin'),'w'); fwrite(fid, [H; W; sigma], 'double'); fclose(fid);

printf('  fftgauss: %dx%d sigma=%d, in[%.2f,%.2f] -> out[%.2f,%.2f]\n', ...
       H, W, sigma, min(map(:)), max(map(:)), min(mapS(:)), max(mapS(:)));
