% Golden for the SNLC lineage's watershed primitive — a faithful port of Octave
% `watershed(A, 4)` (Meyer flooding, 4-connected, watershed lines = 0). This is
% the watershed `resetPatch` uses; it is a DIFFERENT algorithm/oracle from the
% skimage 8-connected `watershed_from_markers` the Allen lineage uses, so it is
% ported and golden-tested separately.
%
% Cases are the authoritative vectors from Octave image's own `watershed.cc-tst`
% (the 5×5 ramp-with-corners and the 5×4 stress matrix), run through the REAL
% Octave `watershed(·,4)`. Matching these bit-for-bit pins the regional-minima
% labeling, the priority/FIFO flooding, and the watershed-line rule.
%
% Outputs (fixtures/, row-major):
%   ws4_<case>_in.bin    (H x W f64) input elevation
%   ws4_<case>_out.bin   (H x W i32) watershed labels (0 = line)
%   ws4_<case>_meta.bin  (f64) [H, W]
% Run:  via `cargo xtask goldens watershed4`

pkg load image;
fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');

function dump(fixdir, name, im)
  L4 = watershed(im, 4);           % REAL Octave watershed, 4-connected
  L8 = watershed(im, 8);           % REAL Octave watershed, 8-connected (default)
  [H, W] = size(im);
  fid = fopen(fullfile(fixdir, ['ws4_' name '_in.bin']),  'w'); fwrite(fid, im',         'double'); fclose(fid);
  fid = fopen(fullfile(fixdir, ['ws4_' name '_out.bin']), 'w'); fwrite(fid, int32(L4'),  'int32');  fclose(fid);
  fid = fopen(fullfile(fixdir, ['ws8_' name '_out.bin']), 'w'); fwrite(fid, int32(L8'),  'int32');  fclose(fid);
  fid = fopen(fullfile(fixdir, ['ws4_' name '_meta.bin']),'w'); fwrite(fid, [H; W],      'double'); fclose(fid);
  printf('  watershed %-6s: %dx%d, max4=%d max8=%d\n', name, H, W, max(L4(:)), max(L8(:)));
end

% Case A — the 5×5 ramp with low corners (watershed.cc-tst).
imA = [
    3     4     5     6     0
    2     3     4     5     6
    1     2     3     4     5
    0     1     2     3     4
    1     0     1     2     3];
dump(fixdir, 'rampA', imA);

% Case B — the 5×4 stress matrix (watershed.cc-tst).
imB = [
    2     3    30     2
    3    30     3    30
  255    31    30     4
    2   255    31    30
    1     2   255     5];
dump(fixdir, 'stress', imB);

% Case C — a distance-transform style case (resetPatch's actual use): two seeds,
% Euclidean distance, so the watershed is the medial split between them.
seeds = zeros(15, 30);
seeds(8, 6) = 1; seeds(8, 24) = 1;
imC = bwdist(seeds);
dump(fixdir, 'distxform', imC);
