% Golden-vector generator for the SNLC cortex morphology primitives.
%
% Validates that our Rust binary morphology (`segmentation/morphology.rs`,
% `connectivity.rs`) reproduces Octave/MATLAB Image Processing Toolbox on the
% exact operations `SnlcGarrett2014ImBound` performs (getMouseAreasX.m):
%   imopen(.,disk2), imclose(.,disk10), imfill(.,'holes'),
%   imdilate(.,disk3), and largest 4-connected component.
%
% Tests each op INDEPENDENTLY on one synthetic mask so any divergence is
% localized. The mask deliberately stresses border handling (a blob on the
% top edge), hole filling, gap bridging (two blobs within a disk-10 close),
% and small-object/keep-largest behavior (isolated specks).
%
% Output (raw uint8, row-major, 96x96): fixtures/cortex_morph_{input,open,
% close,fill,dilate,largestcc}.bin
%
% Run:  octave-cli --norc gen_cortex_morph_golden.m

pkg load image;

N = 96;
[X, Y] = meshgrid(1:N, 1:N);   % X = col index, Y = row index
bw = false(N, N);

% big blob (center-left) with a hole punched out
bw = bw | (((X - 30).^2 + (Y - 50).^2) <= 18^2);
bw(((X - 30).^2 + (Y - 50).^2) <= 6^2) = false;          % interior hole

% blob straddling the TOP border (row 1) — exercises erosion/dilation padding
bw = bw | (((X - 62).^2 + (Y - 2).^2) <= 12^2);

% two blobs with a ~6 px surface gap — imclose(disk10) must bridge them
bw = bw | (((X - 72).^2 + (Y - 64).^2) <= 6^2);
bw = bw | (((X - 72).^2 + (Y - 82).^2) <= 6^2);

% isolated specks — imopen should erase, keep-largest should drop
bw(8, 88) = true;
bw(88, 12) = true;
bw(4, 4) = true;

D2  = strel('disk', 2, 0);
D3  = strel('disk', 3, 0);
D10 = strel('disk', 10, 0);

results = struct();
results.input     = bw;
results.open      = imopen(bw, D2);
results.close     = imclose(bw, D10);
results.fill      = imfill(bw, 'holes');
results.dilate    = imdilate(bw, D3);

% largest connected component, 4-connectivity (SNLC: bwlabel(.,4))
lbl = bwlabel(bw, 4);
n = max(lbl(:));
counts = zeros(n, 1);
for k = 1:n
  counts(k) = sum(lbl(:) == k);
end
[~, biggest] = max(counts);
results.largestcc = (lbl == biggest);

fixdir = fullfile(fileparts(mfilename('fullpath')), 'fixtures');
if ~exist(fixdir, 'dir'); mkdir(fixdir); end

names = fieldnames(results);
for i = 1:numel(names)
  nm = names{i};
  a = uint8(results.(nm));
  % fwrite is column-major; transpose so the bytes are row-major (C-order)
  fid = fopen(fullfile(fixdir, ['cortex_morph_' nm '.bin']), 'w');
  fwrite(fid, a', 'uint8');
  fclose(fid);
  printf('  wrote cortex_morph_%-10s sum=%d\n', [nm '.bin'], sum(a(:)));
end
printf('  grid N=%d, uint8 row-major\n', N);
