//! PNG tEXt chunk writer — insert tEXt chunks into existing PNG files.
//!
//! Inserts new tEXt chunks just before the IEND marker.
//! Computes correct CRC-32 for each chunk (type + data).

use std::fs;
use std::io;
use std::path::Path;

use crate::PNG_SIGNATURE;

/// Write a tEXt chunk into an existing PNG file.
///
/// The chunk is inserted immediately before the IEND marker.
/// CRC-32 is computed per the PNG specification (over type + data).
///
/// # Errors
///
/// Returns an error if the file is not a valid PNG or if I/O fails.
pub fn write_text_chunk(path: &Path, keyword: &str, text: &str) -> io::Result<()> {
    let data = fs::read(path)?;

    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a PNG file"));
    }

    let iend_pos = find_iend_position(&data)?;
    let chunk_bytes = build_text_chunk(keyword, text);

    let mut out = Vec::with_capacity(data.len() + chunk_bytes.len());
    out.extend_from_slice(&data[..iend_pos]);
    out.extend_from_slice(&chunk_bytes);
    out.extend_from_slice(&data[iend_pos..]);

    fs::write(path, &out)
}

/// Find the byte offset where the IEND chunk starts (at its length field).
fn find_iend_position(data: &[u8]) -> io::Result<usize> {
    // Skip PNG signature
    let mut pos = 8;

    while pos + 8 <= data.len() {
        let length = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let chunk_type = &data[pos + 4..pos + 8];

        if chunk_type == b"IEND" {
            return Ok(pos);
        }

        // length(4) + type(4) + data(length) + crc(4)
        let chunk_total = 4 + 4 + length as usize + 4;
        pos = pos
            .checked_add(chunk_total)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "chunk size overflow"))?;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "IEND chunk not found",
    ))
}

/// Build a complete tEXt chunk: length(4) + "tEXt"(4) + data + crc(4).
fn build_text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut chunk_data = Vec::with_capacity(keyword.len() + 1 + text.len());
    chunk_data.extend_from_slice(keyword.as_bytes());
    chunk_data.push(0); // null separator
    chunk_data.extend_from_slice(text.as_bytes());

    let length = chunk_data.len() as u32;

    let mut crc_input = Vec::with_capacity(4 + chunk_data.len());
    crc_input.extend_from_slice(b"tEXt");
    crc_input.extend_from_slice(&chunk_data);
    let crc = crc32(&crc_input);

    let mut buf = Vec::with_capacity(4 + 4 + chunk_data.len() + 4);
    buf.extend_from_slice(&length.to_be_bytes());
    buf.extend_from_slice(b"tEXt");
    buf.extend_from_slice(&chunk_data);
    buf.extend_from_slice(&crc.to_be_bytes());
    buf
}

/// CRC-32 per PNG specification (ISO 3309 / ITU-T V.42).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::read_text_chunks;
    use crate::test_util::make_test_png;

    fn write_test_png(name: &str, chunks: &[(&str, &str)]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, make_test_png(chunks)).unwrap();
        path
    }

    #[test]
    fn write_to_empty_png() {
        let path = write_test_png("pngmeta_write_empty.png", &[]);
        write_text_chunk(&path, "vdsl", r#"{"seed":42}"#).unwrap();

        let chunks = read_text_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks["vdsl"], r#"{"seed":42}"#);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_preserves_existing_chunks() {
        let path = write_test_png("pngmeta_write_preserve.png", &[("prompt", "hello")]);
        write_text_chunk(&path, "vdsl", r#"{"seed":1}"#).unwrap();

        let chunks = read_text_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks["prompt"], "hello");
        assert_eq!(chunks["vdsl"], r#"{"seed":1}"#);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_rejects_non_png() {
        let path = std::env::temp_dir().join("pngmeta_write_not_png.txt");
        std::fs::write(&path, b"not a png").unwrap();

        let result = write_text_chunk(&path, "key", "value");
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_multiple_chunks_sequentially() {
        let path = write_test_png("pngmeta_write_multi.png", &[]);
        write_text_chunk(&path, "alpha", "a").unwrap();
        write_text_chunk(&path, "beta", "b").unwrap();

        let chunks = read_text_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks["alpha"], "a");
        assert_eq!(chunks["beta"], "b");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn crc32_known_value() {
        // "IEND" → 0xAE426082 per PNG spec
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}
