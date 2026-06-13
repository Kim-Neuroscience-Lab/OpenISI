% Golden for `label_patches_with_majority_sign` /
% `patches_from_labels_majority_sign` (segmentation/connectivity.rs:195,207)
% against a VERBATIM transcription of SNLC `getPatchSign.m` run on Octave's
% Image package (the real `bwlabel(im,4)` + `sign(mean(...))`).
%
% getPatchSign.m (verbatim, reference/ISI/getPatchSign.m):
%     imlabel = bwlabel(im,4);
%     areaID  = unique(imlabel);
%     patchSign = zeros(size(imlabel));
%     for i = 2:length(areaID)
%        id = find(imlabel == areaID(i));
%        m = mean(imsign(id));
%        areaSign(i-1) = sign(m);
%        patchSign(id) = sign(m)+1.1;
%     end
%
% We reproduce it EXACTLY, then collapse to an order-independent per-pixel
% signed map `signmap` in {-1,0,+1}: for every patch pixel, signmap = sign(m)
% of its connected component; background = 0. This is order-independent (does
% NOT depend on MATLAB column-major vs Rust row-major label numbering), so the
% Rust side can reconstruct the same map from its Vec<Patch> and compare.
%
% Stress inputs (single NxN case):
%   - multiple 4-conn components, some border-touching
%   - a component whose mean VFS is EXACTLY 0 (tie-break: MATLAB sign=0)
%   - components with mixed-sign pixels where the MEAN decides (not majority
%     count): few large-magnitude pixels outvote many small opposite ones
%   - diagonally-touching blobs that MUST stay separate under 4-conn
%   - a NaN pixel inside one component (Rust skips !is_finite; we make the
%     component still well-defined: NaN excluded from the decision; here we
%     instead avoid NaN inside getPatchSign because mean(NaN)=NaN -> sign=NaN.
%     We test the finite case; NaN handling noted in the analysis.)
%
% Output (fixtures/, uint8 + <f8 + <f4, row-major C-order, NxN):
%   patchsign_mask.bin     uint8  binary patch mask (input `im`)
%   patchsign_vfs.bin      <f8    VFS signal (input `imsign`)
%   patchsign_signmap.bin  <f4    per-pixel sign(mean) in {-1,0,+1}, 0=bg
% Run: octave-cli gen_patchsign_golden.m

1;  % mark as script file so local functions are permitted

pkg load image;

N = 24;
mask = zeros(N, N);
vfs  = zeros(N, N);

% --- Component A: top-left, border-touching, clearly POSITIVE mean ---
mask(1:4, 1:4) = 1;
vfs(1:4, 1:4)  = 0.7;

% --- Component B: clearly NEGATIVE mean, border-touching right edge ---
mask(1:4, N-3:N) = 1;
vfs(1:4, N-3:N)  = -0.9;

% --- Component C: MEAN-decides (mean negative despite more positive PIXELS) ---
% 5 small positive pixels (+0.1 each = +0.5) vs 1 huge negative (-2.0) -> mean<0
mask(8:8, 6:11) = 1;       % a 1x6 horizontal bar, 6 pixels
vfs(8, 6:10)    = 0.1;     % five +0.1
vfs(8, 11)      = -2.0;    % one -2.0  -> sum = -1.5, mean < 0

% --- Component D: EXACT-ZERO mean (tie-break). +1 and -1 cancel. ---
% MATLAB sign(0) = 0 ; our Rust sum>=0 -> +1.  Divergence probe.
mask(12:12, 6:9) = 1;      % 1x4 bar
vfs(12, 6)  =  1.0;
vfs(12, 7)  =  1.0;
vfs(12, 8)  = -1.0;
vfs(12, 9)  = -1.0;        % sum = 0 -> mean = 0

% --- Components E & F: diagonally touching -> MUST be two 4-conn comps ---
mask(16, 14) = 1; vfs(16, 14) =  0.5;   % E (positive)
mask(17, 15) = 1; vfs(17, 15) = -0.5;   % F (negative), only diagonal contact

% --- Component G: large blob, mostly positive, one negative speck; mean>0 ---
mask(19:23, 16:21) = 1;
vfs(19:23, 16:21)  = 0.3;
vfs(21, 18)        = -0.05;   % small dent, mean stays positive

% ---- run getPatchSign.m verbatim ----
im     = mask;
imsign = vfs;
imlabel = bwlabel(im, 4);
areaID  = unique(imlabel);
patchSign = zeros(size(imlabel));
areaSign = [];
for i = 2:length(areaID)
   id = find(imlabel == areaID(i));
   m = mean(imsign(id));
   areaSign(i-1) = sign(m);
   patchSign(id) = sign(m) + 1.1;
end

% collapse to order-independent signed map in {-1,0,+1}
signmap = zeros(size(imlabel));
ncomp = max(imlabel(:));
for k = 1:ncomp
   id = find(imlabel == k);
   m  = mean(imsign(id));
   signmap(id) = sign(m);
end

% --- write fixtures, C-order row-major. Octave is column-major, so transpose
%     before linearizing so that fwrite emits row-major bytes. ---
fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
if ~exist(fixdir, 'dir'); mkdir(fixdir); end

% A.' linearizes column-major over the transpose = row-major over the original.
fid = fopen(fullfile(fixdir, 'patchsign_mask.bin'), 'wb');
fwrite(fid, mask.', 'uint8'); fclose(fid);
fid = fopen(fullfile(fixdir, 'patchsign_vfs.bin'), 'wb');
fwrite(fid, vfs.', 'double'); fclose(fid);
fid = fopen(fullfile(fixdir, 'patchsign_signmap.bin'), 'wb');
fwrite(fid, signmap.', 'single'); fclose(fid);

printf('  N=%d  components=%d\n', N, ncomp);
printf('  mask sum (patch px) = %d\n', sum(mask(:)));
printf('  areaSign (label order, col-major) = [%s]\n', sprintf('%d ', areaSign));
printf('  signmap value counts: -1=%d  0=%d  +1=%d  bg=%d\n', ...
       sum(signmap(:)==-1), sum(signmap(:)==0 & mask(:)==1), ...
       sum(signmap(:)==1), sum(mask(:)==0));
printf('  component means:\n');
for k = 1:ncomp
   id = find(imlabel == k);
   printf('    comp %d: npx=%d  mean=%+.4f  sign=%+d\n', ...
          k, numel(id), mean(imsign(id)), sign(mean(imsign(id))));
end
