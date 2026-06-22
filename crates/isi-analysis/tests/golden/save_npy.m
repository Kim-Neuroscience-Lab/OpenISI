function save_npy(filename, arr, descr)
% SAVE_NPY  Write a 1-D or 2-D array as a NumPy .npy file (format v1.0), C-order.
%
%   save_npy(filename, arr, descr)
%
% descr is the NumPy dtype string: '<f8' (double), '<f4' (single),
% '|u1' (uint8), '<i4' (int32), '<u2' (uint16).
%
% The committed golden fixtures are .npy so the dtype + shape travel WITH the
% data (self-describing), and the Rust readers verify the dtype on load. This is
% the Octave counterpart of numpy's `np.save`. Octave stores column-major, so a
% 2-D array is transposed before writing to land the data in C (row-major) order
% -- matching `fortran_order: False` in the header and the row-major order the
% Rust `load_*` helpers (and numpy) expect.

  if isvector(arr)
    shape_str = sprintf('(%d,)', numel(arr));
    data = arr(:);                 % flat, order-independent for a vector
  else
    sz = size(arr);
    shape_str = sprintf('(%d, %d)', sz(1), sz(2));
    tarr = arr.';                  % transpose: column-major(arr') == row-major(arr)
    data = tarr(:);                % → C-order bytes
  end

  hdr = sprintf("{'descr': '%s', 'fortran_order': False, 'shape': %s, }", descr, shape_str);
  % Pad with spaces + a trailing newline so (10 prelude + header length) is a
  % multiple of 64, per the .npy spec alignment requirement.
  prelude = 10;                    % 6 magic + 2 version + 2 header-length
  pad = mod(64 - mod(prelude + numel(hdr) + 1, 64), 64);
  hdr = [hdr repmat(' ', 1, pad) char(10)];

  fid = fopen(filename, 'wb');
  if fid < 0
    error('save_npy: cannot open %s', filename);
  end
  fwrite(fid, [char(147) 'NUMPY'], 'uint8');   % magic \x93NUMPY
  fwrite(fid, [1 0], 'uint8');                 % version 1.0
  fwrite(fid, numel(hdr), 'uint16');           % HEADER_LEN (little-endian on x86)
  fwrite(fid, hdr, 'uint8');
  switch descr
    case '<f8'; fwrite(fid, data, 'double');
    case '<f4'; fwrite(fid, data, 'single');
    case '|u1'; fwrite(fid, data, 'uint8');
    case '<i4'; fwrite(fid, data, 'int32');
    case '<u2'; fwrite(fid, data, 'uint16');
    otherwise;  fclose(fid); error('save_npy: unknown descr %s', descr);
  end
  fclose(fid);
end
