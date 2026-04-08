//! Minimal MAT v5 binary reader — focused on reading the SNLC data pattern:
//! a variable `f1m` containing a 2-element cell array of complex double matrices.
//!
//! References:
//!   MAT-File Format R2024a documentation (mathworks.com)
//!
//! Only handles: little-endian files, miCOMPRESSED top-level elements,
//! miMATRIX (class cell, class double), miDOUBLE/miSINGLE numeric data.

use flate2::read::ZlibDecoder;
use ndarray::Array2;
use num_complex::Complex64;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::AnalysisError;

// ---------------------------------------------------------------------------
// MAT v5 type tags
// ---------------------------------------------------------------------------

const MI_INT8: u32 = 1;
const MI_UINT8: u32 = 2;
const MI_INT16: u32 = 3;
const MI_UINT16: u32 = 4;
const MI_INT32: u32 = 5;
const MI_UINT32: u32 = 6;
const MI_SINGLE: u32 = 7;
const MI_DOUBLE: u32 = 9;
const MI_INT64: u32 = 12;
const MI_UINT64: u32 = 13;
const MI_MATRIX: u32 = 14;
const MI_COMPRESSED: u32 = 15;
const _MI_UTF8: u32 = 16;

// Array class codes (within miMATRIX flags)
const MX_CELL_CLASS: u8 = 1;
const MX_STRUCT_CLASS: u8 = 2;
const MX_DOUBLE_CLASS: u8 = 6;
const MX_SINGLE_CLASS: u8 = 7;
const MX_INT8_CLASS: u8 = 8;
const MX_UINT8_CLASS: u8 = 9;
const MX_INT16_CLASS: u8 = 10;
const MX_UINT16_CLASS: u8 = 11;
const MX_INT32_CLASS: u8 = 12;
const MX_UINT32_CLASS: u8 = 13;
const MX_INT64_CLASS: u8 = 14;
const MX_UINT64_CLASS: u8 = 15;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A complex 2D matrix read from a .mat file.
pub struct MatComplex2D {
    pub data: Array2<Complex64>,
}

/// Read the variable `f1m` from an SNLC .mat file.
/// Returns a Vec of complex matrices (expected to be 2 — forward and reverse).
pub fn read_snlc_f1m(path: &Path) -> Result<Vec<MatComplex2D>, AnalysisError> {
    let bytes = std::fs::read(path)
        .map_err(|e| AnalysisError::Io(e))?;

    if bytes.len() < 128 {
        return Err(mat_err("file too small for MAT v5 header"));
    }

    // Validate header
    let header_text = &bytes[0..116];
    if !header_text.starts_with(b"MATLAB") {
        return Err(mat_err("not a MATLAB file (bad magic)"));
    }

    // Bytes 124-125: version (0x0100), bytes 126-127: endian indicator
    let endian = &bytes[126..128];
    if endian != b"IM" {
        return Err(mat_err("only little-endian MAT files are supported"));
    }

    let mut cursor = Cursor::new(&bytes[..]);
    cursor.seek(SeekFrom::Start(128))?;

    // Read top-level data elements, looking for variable named "f1m"
    while (cursor.position() as usize) < bytes.len() {
        let pos = cursor.position() as usize;
        if pos + 8 > bytes.len() {
            break;
        }

        let (dtype, size) = read_tag(&mut cursor)?;

        match dtype {
            MI_COMPRESSED => {
                // Decompress and parse the contained miMATRIX
                let compressed = read_n_bytes(&mut cursor, size as usize)?;
                let decompressed = zlib_decompress(&compressed)?;
                let mut inner = Cursor::new(&decompressed[..]);

                if let Some(result) = try_read_f1m_matrix(&mut inner)? {
                    return Ok(result);
                }
            }
            MI_MATRIX => {
                // Uncompressed miMATRIX
                let start = cursor.position() as usize;
                let matrix_bytes = &bytes[start..start + size as usize];
                let mut inner = Cursor::new(matrix_bytes);

                // Parse the matrix, checking if it's named "f1m"
                let (name, parsed) = parse_matrix_with_name(&mut inner, size as usize)?;
                if name == "f1m" {
                    if let MatrixData::Cell(cells) = parsed {
                        return cells_to_complex_matrices(cells);
                    } else {
                        return Err(mat_err("f1m is not a cell array"));
                    }
                }

                cursor.seek(SeekFrom::Start((start + size as usize) as u64))?;
            }
            _ => {
                // Skip unknown top-level element
                skip_padded(&mut cursor, size)?;
            }
        }
    }

    Err(mat_err("variable 'f1m' not found in .mat file"))
}

/// Read an anatomical grayscale image from an SNLC grab_ .mat file.
/// The grab file typically contains a struct with a numeric 2D array field (the image).
/// This function searches recursively through all variables and struct fields.
pub fn read_snlc_anatomical(path: &Path) -> Result<Array2<u8>, AnalysisError> {
    let bytes = std::fs::read(path)
        .map_err(|e| AnalysisError::Io(e))?;

    if bytes.len() < 128 || !bytes.starts_with(b"MATLAB") {
        return Err(mat_err("not a MATLAB file"));
    }
    if &bytes[126..128] != b"IM" {
        return Err(mat_err("only little-endian MAT files supported"));
    }

    let mut cursor = Cursor::new(&bytes[..]);
    cursor.seek(SeekFrom::Start(128))?;

    while (cursor.position() as usize) < bytes.len() {
        if cursor.position() as usize + 8 > bytes.len() {
            break;
        }

        let (dtype, size) = read_tag(&mut cursor)?;

        match dtype {
            MI_COMPRESSED => {
                let compressed = read_n_bytes(&mut cursor, size as usize)?;
                let decompressed = zlib_decompress(&compressed)?;
                let mut inner = Cursor::new(&decompressed[..]);
                let (inner_type, inner_size) = read_tag(&mut inner)?;
                if inner_type == MI_MATRIX {
                    let (_, data) = parse_matrix_contents(&mut inner, inner_size as usize)?;
                    if let Some(image) = extract_2d_image(&data) {
                        return Ok(image);
                    }
                }
            }
            MI_MATRIX => {
                let (_, data) = parse_matrix_contents(&mut cursor, size as usize)?;
                if let Some(image) = extract_2d_image(&data) {
                    return Ok(image);
                }
            }
            _ => {
                skip_padded(&mut cursor, size)?;
            }
        }
    }

    Err(mat_err("no 2D numeric array found in anatomical .mat file"))
}

/// Recursively search a MatrixData tree for the largest 2D numeric array.
fn extract_2d_image(data: &MatrixData) -> Option<Array2<u8>> {
    match data {
        MatrixData::Double { dims, real, .. } => {
            if dims.len() == 2 && dims[0] > 1 && dims[1] > 1 {
                let (h, w) = (dims[0], dims[1]);
                if real.len() == h * w {
                    // Auto-contrast: find min/max and scale to 0-255.
                    let mut min_val = f64::INFINITY;
                    let mut max_val = f64::NEG_INFINITY;
                    for &v in real {
                        if v.is_finite() {
                            if v < min_val { min_val = v; }
                            if v > max_val { max_val = v; }
                        }
                    }
                    let range = (max_val - min_val).max(1e-10);

                    let mut result = Array2::<u8>::zeros((h, w));
                    for r in 0..h {
                        for c in 0..w {
                            let val = real[c * h + r]; // column-major
                            let normalized = ((val - min_val) / range * 255.0).clamp(0.0, 255.0);
                            result[[r, c]] = normalized as u8;
                        }
                    }
                    return Some(result);
                }
            }
            None
        }
        MatrixData::Struct { fields } => {
            // Search struct fields for the largest 2D numeric array
            let mut best: Option<Array2<u8>> = None;
            for field in fields {
                if let Some(img) = extract_2d_image(field) {
                    let size = img.len();
                    if best.as_ref().map_or(true, |b| size > b.len()) {
                        best = Some(img);
                    }
                }
            }
            best
        }
        MatrixData::Cell(cells) => {
            for cell in cells {
                if let Some(img) = extract_2d_image(cell) {
                    return Some(img);
                }
            }
            None
        }
        MatrixData::Unknown => None,
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

enum MatrixData {
    Cell(Vec<MatrixData>),
    Struct {
        fields: Vec<MatrixData>,
    },
    Double {
        dims: Vec<usize>,
        real: Vec<f64>,
        imag: Option<Vec<f64>>,
    },
    Unknown,
}

// ---------------------------------------------------------------------------
// Tag reading
// ---------------------------------------------------------------------------

/// Read a MAT v5 tag. Handles both regular (8-byte) and "small data element" (4-byte) formats.
/// Returns (type, size). For small data elements, the data follows immediately in the tag bytes.
fn read_tag<R: Read + Seek>(r: &mut R) -> Result<(u32, u32), AnalysisError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;

    let first_u32 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let second_u32 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

    // Check for "small data element" format: if the upper 2 bytes of the first u32 are non-zero
    let upper_two = (first_u32 >> 16) & 0xFFFF;
    if upper_two != 0 {
        // Small data format: lower 2 bytes = type, upper 2 bytes = size
        let dtype = first_u32 & 0xFFFF;
        let size = upper_two;
        // Data is in the next 4 bytes (second_u32), but we've already read them.
        // Seek back 4 so caller can read the data portion.
        r.seek(SeekFrom::Current(-4))?;
        Ok((dtype, size))
    } else {
        // Regular format: first u32 = type, second u32 = size
        Ok((first_u32, second_u32))
    }
}

/// Read a tag and its data as bytes. Handles both small and regular formats.
fn read_tag_data<R: Read + Seek>(r: &mut R) -> Result<(u32, Vec<u8>), AnalysisError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;

    let first_u32 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let second_u32 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

    let upper_two = (first_u32 >> 16) & 0xFFFF;
    if upper_two != 0 {
        // Small data format: data is in buf[4..4+size]
        let dtype = first_u32 & 0xFFFF;
        let size = upper_two as usize;
        let data = buf[4..4 + size.min(4)].to_vec();
        Ok((dtype, data))
    } else {
        // Regular format
        let dtype = first_u32;
        let size = second_u32 as usize;
        let data = read_n_bytes(r, size)?;
        // Pad to 8-byte boundary
        let padded = pad8(size);
        if padded > size {
            let skip = (padded - size) as i64;
            r.seek(SeekFrom::Current(skip))?;
        }
        Ok((dtype, data))
    }
}

// ---------------------------------------------------------------------------
// Matrix parsing
// ---------------------------------------------------------------------------

/// Try to read a top-level element as miMATRIX named "f1m".
/// Returns Some(cells) if found, None if this isn't "f1m".
fn try_read_f1m_matrix<R: Read + Seek>(r: &mut R) -> Result<Option<Vec<MatComplex2D>>, AnalysisError> {
    let (dtype, size) = read_tag(r)?;
    if dtype != MI_MATRIX {
        return Ok(None);
    }

    let start = r.stream_position()? as usize;
    let (name, parsed) = parse_matrix_contents(r, size as usize)?;

    if name == "f1m" {
        if let MatrixData::Cell(cells) = parsed {
            return Ok(Some(cells_to_complex_matrices(cells)?));
        } else {
            return Err(mat_err("f1m is not a cell array"));
        }
    }

    // Seek past this matrix
    r.seek(SeekFrom::Start((start + size as usize) as u64))?;
    Ok(None)
}

/// Parse the contents of an miMATRIX element (after the tag).
fn parse_matrix_contents<R: Read + Seek>(r: &mut R, total_size: usize) -> Result<(String, MatrixData), AnalysisError> {
    let end_pos = r.stream_position()? as usize + total_size;

    // Sub-element 1: Array Flags (miUINT32, 8 bytes of data)
    let (_, flags_data) = read_tag_data(r)?;
    if flags_data.len() < 4 {
        return Ok(("".into(), MatrixData::Unknown));
    }
    let flags = u32::from_le_bytes([flags_data[0], flags_data[1], flags_data[2], flags_data[3]]);
    let array_class = (flags & 0xFF) as u8;
    let is_complex = (flags & 0x0800) != 0;

    // Sub-element 2: Dimensions Array (miINT32)
    let (_, dims_data) = read_tag_data(r)?;
    let n_dims = dims_data.len() / 4;
    let dims: Vec<usize> = (0..n_dims)
        .map(|i| {
            let off = i * 4;
            i32::from_le_bytes([
                dims_data[off], dims_data[off + 1],
                dims_data[off + 2], dims_data[off + 3],
            ]) as usize
        })
        .collect();

    // Sub-element 3: Array Name (miINT8)
    let (_, name_data) = read_tag_data(r)?;
    let name = String::from_utf8_lossy(&name_data).trim_end_matches('\0').to_string();

    match array_class {
        MX_CELL_CLASS => {
            // Cell array: remaining sub-elements are miMATRIX entries
            let n_cells: usize = dims.iter().product();
            let mut cells = Vec::with_capacity(n_cells);
            for _ in 0..n_cells {
                if (r.stream_position()? as usize) >= end_pos {
                    break;
                }
                let (child_type, child_size) = read_tag(r)?;
                if child_type == MI_MATRIX {
                    let (_, child_data) = parse_matrix_contents(r, child_size as usize)?;
                    cells.push(child_data);
                } else {
                    skip_padded(r, child_size)?;
                    cells.push(MatrixData::Unknown);
                }
            }
            Ok((name, MatrixData::Cell(cells)))
        }
        MX_STRUCT_CLASS => {
            // Struct: field name length, field names, then N*num_fields miMATRIX values
            // Sub-element: field name length (miINT32, small data format usually)
            let (_, fnl_data) = read_tag_data(r)?;
            let field_name_len = if fnl_data.len() >= 4 {
                i32::from_le_bytes([fnl_data[0], fnl_data[1], fnl_data[2], fnl_data[3]]) as usize
            } else {
                32 // default
            };

            // Sub-element: concatenated field names (miINT8), each `field_name_len` bytes
            let (_, fn_data) = read_tag_data(r)?;
            let num_fields = if field_name_len > 0 { fn_data.len() / field_name_len } else { 0 };

            // Read field values (for each struct element × each field)
            let n_elements: usize = dims.iter().product();
            let total_fields = n_elements * num_fields;
            let mut fields = Vec::with_capacity(total_fields);
            for _ in 0..total_fields {
                if (r.stream_position()? as usize) >= end_pos {
                    break;
                }
                let (child_type, child_size) = read_tag(r)?;
                if child_type == MI_MATRIX {
                    let (_, child_data) = parse_matrix_contents(r, child_size as usize)?;
                    fields.push(child_data);
                } else {
                    skip_padded(r, child_size)?;
                    fields.push(MatrixData::Unknown);
                }
            }

            Ok((name, MatrixData::Struct { fields }))
        }
        MX_DOUBLE_CLASS | MX_SINGLE_CLASS |
        MX_INT8_CLASS | MX_UINT8_CLASS |
        MX_INT16_CLASS | MX_UINT16_CLASS |
        MX_INT32_CLASS | MX_UINT32_CLASS |
        MX_INT64_CLASS | MX_UINT64_CLASS => {
            // Numeric array: next sub-element(s) are real data, optionally imaginary
            let real = read_numeric_subelement(r)?;
            let imag = if is_complex && (r.stream_position()? as usize) < end_pos {
                Some(read_numeric_subelement(r)?)
            } else {
                None
            };
            Ok((name, MatrixData::Double { dims, real, imag }))
        }
        _ => {
            // Unknown class — skip to end
            let current = r.stream_position()? as usize;
            if end_pos > current {
                r.seek(SeekFrom::Start(end_pos as u64))?;
            }
            Ok((name, MatrixData::Unknown))
        }
    }
}

/// Parse for the `parse_matrix_with_name` path (used when we've already read a tag).
fn parse_matrix_with_name<R: Read + Seek>(r: &mut R, size: usize) -> Result<(String, MatrixData), AnalysisError> {
    parse_matrix_contents(r, size)
}

/// Read a numeric sub-element as f64 values.
fn read_numeric_subelement<R: Read + Seek>(r: &mut R) -> Result<Vec<f64>, AnalysisError> {
    let (dtype, data) = read_tag_data(r)?;
    bytes_to_f64(&data, dtype)
}

/// Convert raw bytes to f64 based on MAT type tag.
fn bytes_to_f64(data: &[u8], dtype: u32) -> Result<Vec<f64>, AnalysisError> {
    match dtype {
        MI_DOUBLE => {
            let n = data.len() / 8;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 8;
                out.push(f64::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                    data[off + 4], data[off + 5], data[off + 6], data[off + 7],
                ]));
            }
            Ok(out)
        }
        MI_SINGLE => {
            let n = data.len() / 4;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 4;
                out.push(f32::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                ]) as f64);
            }
            Ok(out)
        }
        MI_INT8 | MI_UINT8 => {
            Ok(data.iter().map(|&b| b as f64).collect())
        }
        MI_INT16 => {
            let n = data.len() / 2;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 2;
                out.push(i16::from_le_bytes([data[off], data[off + 1]]) as f64);
            }
            Ok(out)
        }
        MI_UINT16 => {
            let n = data.len() / 2;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 2;
                out.push(u16::from_le_bytes([data[off], data[off + 1]]) as f64);
            }
            Ok(out)
        }
        MI_INT32 => {
            let n = data.len() / 4;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 4;
                out.push(i32::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                ]) as f64);
            }
            Ok(out)
        }
        MI_UINT32 => {
            let n = data.len() / 4;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 4;
                out.push(u32::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                ]) as f64);
            }
            Ok(out)
        }
        MI_INT64 => {
            let n = data.len() / 8;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 8;
                out.push(i64::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                    data[off + 4], data[off + 5], data[off + 6], data[off + 7],
                ]) as f64);
            }
            Ok(out)
        }
        MI_UINT64 => {
            let n = data.len() / 8;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let off = i * 8;
                out.push(u64::from_le_bytes([
                    data[off], data[off + 1], data[off + 2], data[off + 3],
                    data[off + 4], data[off + 5], data[off + 6], data[off + 7],
                ]) as f64);
            }
            Ok(out)
        }
        _ => Err(mat_err(&format!("unsupported numeric data type tag: {dtype}"))),
    }
}

// ---------------------------------------------------------------------------
// Cell → ComplexMaps conversion
// ---------------------------------------------------------------------------

fn cells_to_complex_matrices(cells: Vec<MatrixData>) -> Result<Vec<MatComplex2D>, AnalysisError> {
    let mut result = Vec::with_capacity(cells.len());
    for (i, cell) in cells.into_iter().enumerate() {
        match cell {
            MatrixData::Double { dims, real, imag } => {
                if dims.len() != 2 {
                    return Err(mat_err(&format!("f1m cell {i}: expected 2D, got {}D", dims.len())));
                }
                let (h, w) = (dims[0], dims[1]);
                let expected = h * w;
                if real.len() != expected {
                    return Err(mat_err(&format!(
                        "f1m cell {i}: expected {expected} elements, got {}",
                        real.len()
                    )));
                }

                let imag_data = imag.ok_or_else(|| {
                    mat_err(&format!("f1m cell {i}: expected complex data but no imaginary part"))
                })?;
                if imag_data.len() != expected {
                    return Err(mat_err(&format!(
                        "f1m cell {i}: imag length {} != expected {expected}",
                        imag_data.len()
                    )));
                }

                // MAT stores column-major (Fortran order): data[col * H + row]
                let mut data = Array2::<Complex64>::zeros((h, w));
                for r in 0..h {
                    for c in 0..w {
                        let idx = c * h + r; // column-major index
                        data[[r, c]] = Complex64::new(real[idx], imag_data[idx]);
                    }
                }
                result.push(MatComplex2D { data });
            }
            _ => {
                return Err(mat_err(&format!("f1m cell {i}: not a numeric double array")));
            }
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

fn read_n_bytes<R: Read>(r: &mut R, n: usize) -> Result<Vec<u8>, AnalysisError> {
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn skip_padded<R: Read + Seek>(r: &mut R, size: u32) -> Result<(), AnalysisError> {
    let padded = pad8(size as usize) as i64;
    r.seek(SeekFrom::Current(padded))?;
    Ok(())
}

/// Round up to next 8-byte boundary.
fn pad8(n: usize) -> usize {
    (n + 7) & !7
}

fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, AnalysisError> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)
        .map_err(|e| AnalysisError::Io(io::Error::new(io::ErrorKind::InvalidData,
            format!("zlib decompression failed: {e}"))))?;
    Ok(out)
}

fn mat_err(msg: &str) -> AnalysisError {
    AnalysisError::InvalidPackage(format!("MAT v5: {msg}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad8() {
        assert_eq!(pad8(0), 0);
        assert_eq!(pad8(1), 8);
        assert_eq!(pad8(7), 8);
        assert_eq!(pad8(8), 8);
        assert_eq!(pad8(9), 16);
    }

    #[test]
    fn test_bytes_to_f64_double() {
        let val: f64 = 3.14;
        let bytes = val.to_le_bytes();
        let result = bytes_to_f64(&bytes, MI_DOUBLE).unwrap();
        assert_eq!(result.len(), 1);
        assert!((result[0] - 3.14).abs() < 1e-15);
    }

    #[test]
    fn test_bytes_to_f64_single() {
        let val: f32 = 2.5;
        let bytes = val.to_le_bytes();
        let result = bytes_to_f64(&bytes, MI_SINGLE).unwrap();
        assert_eq!(result.len(), 1);
        assert!((result[0] - 2.5).abs() < 1e-6);
    }

    #[test]
    fn test_bytes_to_f64_uint8() {
        let bytes = vec![0u8, 128, 255];
        let result = bytes_to_f64(&bytes, MI_UINT8).unwrap();
        assert_eq!(result, vec![0.0, 128.0, 255.0]);
    }

    /// Integration test: read actual SNLC R43 sample data if present.
    /// Set OPENISI_TEST_DATA to the test_data directory path to run this test,
    /// e.g. OPENISI_TEST_DATA=/path/to/test_data cargo test
    #[test]
    fn test_read_r43_horizontal() {
        let base = match std::env::var("OPENISI_TEST_DATA") {
            Ok(dir) => std::path::PathBuf::from(dir),
            Err(_) => {
                eprintln!("Skipping test_read_r43_horizontal: OPENISI_TEST_DATA not set");
                return;
            }
        };
        let path = base.join("snlc_sample_data/R43/R43_000_004.mat");
        if !path.exists() {
            eprintln!("Skipping test_read_r43_horizontal: sample data not found");
            return;
        }
        let cells = read_snlc_f1m(&path).expect("failed to read R43_000_004.mat");
        assert_eq!(cells.len(), 2, "expected 2 complex matrices (fwd + rev)");

        let (h0, w0) = cells[0].data.dim();
        let (h1, w1) = cells[1].data.dim();
        assert_eq!((h0, w0), (h1, w1), "both matrices should have same dimensions");
        assert!(h0 > 0 && w0 > 0, "matrices should be non-empty");

        eprintln!("R43_000_004.mat: 2 complex matrices of size ({h0}, {w0})");

        // Verify data is actually complex (not all zeros)
        let has_nonzero_imag = cells[0].data.iter().any(|z| z.im.abs() > 1e-20);
        assert!(has_nonzero_imag, "expected complex data with nonzero imaginary parts");
    }

    #[test]
    fn test_read_r43_anatomical() {
        let base = match std::env::var("OPENISI_TEST_DATA") {
            Ok(dir) => std::path::PathBuf::from(dir),
            Err(_) => {
                eprintln!("Skipping test_read_r43_anatomical: OPENISI_TEST_DATA not set");
                return;
            }
        };
        let path = base.join("snlc_sample_data/R43/grab_r43_000_006_26_Jul_2012_19_02_23.mat");
        if !path.exists() {
            eprintln!("Skipping test_read_r43_anatomical: sample data not found");
            return;
        }

        // Debug: use f1m reader to just see what variables are in the file
        let bytes = std::fs::read(&path).unwrap();
        let mut cursor = Cursor::new(&bytes[..]);
        cursor.seek(SeekFrom::Start(128)).unwrap();
        while (cursor.position() as usize) < bytes.len() {
            if cursor.position() as usize + 8 > bytes.len() { break; }
            let (dtype, size) = read_tag(&mut cursor).unwrap();
            eprintln!("Top-level element: type={dtype}, size={size}, pos={}", cursor.position());
            if dtype == MI_COMPRESSED {
                let compressed = read_n_bytes(&mut cursor, size as usize).unwrap();
                let decompressed = zlib_decompress(&compressed).unwrap();
                eprintln!("  Decompressed {} bytes → {} bytes", size, decompressed.len());
                let mut inner = Cursor::new(&decompressed[..]);
                let (inner_type, inner_size) = read_tag(&mut inner).unwrap();
                eprintln!("  Inner element: type={inner_type}, size={inner_size}");
                if inner_type == MI_MATRIX {
                    // Peek at flags to get the class code
                    let flag_pos = inner.position();
                    let (_, flag_bytes) = read_tag_data(&mut inner).unwrap();
                    let flag_val = u32::from_le_bytes([flag_bytes[0], flag_bytes[1], flag_bytes[2], flag_bytes[3]]);
                    let class_code = (flag_val & 0xFF) as u8;
                    eprintln!("  Array class code: {class_code}, flags: 0x{flag_val:08x}");
                    inner.seek(SeekFrom::Start(flag_pos)).unwrap();

                    let (name, data) = parse_matrix_contents(&mut inner, inner_size as usize).unwrap();
                    match &data {
                        MatrixData::Double { dims, real, imag } => {
                            eprintln!("  Matrix '{}': dims={:?}, real_len={}, has_imag={}", name, dims, real.len(), imag.is_some());
                        }
                        MatrixData::Cell(cells) => {
                            eprintln!("  Cell '{}': {} cells", name, cells.len());
                        }
                        MatrixData::Struct { fields } => {
                            eprintln!("  Struct '{}': {} fields", name, fields.len());
                        }
                        MatrixData::Unknown => {
                            eprintln!("  Unknown '{}' (class={})", name, class_code);
                        }
                    }
                }
            } else {
                skip_padded(&mut cursor, size).unwrap();
            }
        }

        let anat = read_snlc_anatomical(&path).expect("failed to read grab_ file");
        let (h, w) = anat.dim();
        assert!(h > 0 && w > 0, "anatomical should be non-empty");
        eprintln!("R43 anatomical: ({h}, {w})");

        // Should have some variation (not all same value)
        let min = *anat.iter().min().unwrap();
        let max = *anat.iter().max().unwrap();
        assert!(max > min, "anatomical should have pixel variation");
    }
}
