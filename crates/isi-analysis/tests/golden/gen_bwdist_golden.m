% Golden for `bwdist` — Euclidean distance from each pixel to the nearest
% nonzero (seed) pixel. `resetPatch` (the SNLC split) builds its watershed
% elevation from `bwdist(impatch)`, so this is a needed primitive.
%
% Calls the REAL Octave `bwdist` directly (standard IPT function, no plotting).
% Synthetic: a few scattered seed pixels so the distance field has several
% catchment regions (the structure the split watershed keys on).
%
% Outputs (fixtures/, row-major):
%   bwdist_seeds.bin  (H x W uint8) seed mask (0/1)
%   bwdist_d.bin      (H x W f64)   Euclidean distance to nearest seed (oracle)
%   bwdist_meta.bin   (f64) [H, W]
% Run:  via `cargo xtask goldens bwdist`

pkg load image;

H = 40; W = 40;
BW = zeros(H, W);
BW(10, 10) = 1; BW(30, 25) = 1; BW(15, 35) = 1; BW(35, 8) = 1;  % scattered seeds

D = bwdist(BW);   % REAL oracle — Euclidean to nearest nonzero

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
fid = fopen(fullfile(fixdir, 'bwdist_seeds.bin'), 'w'); fwrite(fid, uint8(BW'), 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'bwdist_d.bin'), 'w');     fwrite(fid, D', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'bwdist_meta.bin'), 'w');  fwrite(fid, [H; W], 'double'); fclose(fid);

printf('  bwdist: %d seeds, max dist=%.4f\n', sum(BW(:)), max(D(:)));
