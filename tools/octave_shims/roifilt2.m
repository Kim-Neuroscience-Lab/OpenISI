function J = roifilt2(h, I, BW)
% ROIFILT2  Faithful Octave shim for the MATLAB Image-Processing-Toolbox
% function (Octave's `image` package does not implement it).
%
% Implements the 3-argument linear-filter form used by the SNLC oracle
% (`reference/ISI/smoothPatchesX.m`):
%
%   J = roifilt2(h, I, BW)
%
% Per the MATLAB documentation: filter the data in I with the 2-D linear
% filter h, returning filtered values for pixels where BW is nonzero and the
% original (unfiltered) I elsewhere. MATLAB uses `filter2` (zero-padded,
% 'same' size) for the filtering; the SNLC filter h is a symmetric
% `fspecial('gaussian', ...)`, for which correlation == convolution.
%
% This is a missing-library-primitive compatibility layer (like the `image`
% package's `fspecial`/`strel`), NOT a change to the oracle's logic — it lives
% outside `reference/ISI` so the oracle source stays pristine.

  filtered = filter2(h, I);
  J = I;
  mask = BW ~= 0;
  J(mask) = filtered(mask);
end
