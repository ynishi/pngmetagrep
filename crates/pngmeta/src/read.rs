//! PNG tEXt chunk extraction — streaming, no image decoding.
//!
//! PNG structure: 8-byte signature, then chunks of
//! length(4 BE) + type(4) + data(length) + crc(4).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::PNG_SIGNATURE;

/// Extract all tEXt chunks from a PNG file.
///
/// Returns keyword → text pairs in sorted order. Reads chunk headers
/// sequentially; non-tEXt data is seeked over without loading into memory.
pub fn read_text_chunks(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut sig = [0u8; 8];
    reader.read_exact(&mut sig)?;
    if sig != PNG_SIGNATURE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a PNG file"));
    }

    let mut chunks = BTreeMap::new();
    let mut header = [0u8; 8];

    while reader.read_exact(&mut header).is_ok() {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);

        if &header[4..8] == b"tEXt" {
            let mut data = vec![0u8; length as usize];
            reader.read_exact(&mut data)?;
            reader.seek(SeekFrom::Current(4))?; // skip CRC

            if let Some(null_pos) = data.iter().position(|&b| b == 0) {
                let keyword = String::from_utf8_lossy(&data[..null_pos]).into_owned();
                let text = String::from_utf8_lossy(&data[null_pos + 1..]).into_owned();
                chunks.insert(keyword, text);
            }
        } else if &header[4..8] == b"IEND" {
            break;
        } else {
            reader.seek(SeekFrom::Current(i64::from(length) + 4))?;
        }
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::make_test_png;
    use std::path::PathBuf;

    fn write_test_png(name: &str, chunks: &[(&str, &str)]) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, make_test_png(chunks)).unwrap();
        path
    }

    #[test]
    fn rejects_non_png() {
        let path = std::env::temp_dir().join("pngmeta_test_not_a_png.txt");
        std::fs::write(&path, b"hello world").unwrap();
        let result = read_text_chunks(&path);
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_missing_file() {
        let result = read_text_chunks(Path::new("/nonexistent/file.png"));
        assert!(result.is_err());
    }

    #[test]
    fn extracts_single_text_chunk() {
        let path = write_test_png("pngmeta_test_single.png", &[("vdsl", r#"{"seed":42}"#)]);
        let chunks = read_text_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks["vdsl"], r#"{"seed":42}"#);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extracts_multiple_text_chunks() {
        let path = write_test_png(
            "pngmeta_test_multi.png",
            &[("prompt", "hello"), ("workflow", r#"{"nodes":[]}"#)],
        );
        let chunks = read_text_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks["prompt"], "hello");
        assert_eq!(chunks["workflow"], r#"{"nodes":[]}"#);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn returns_empty_for_png_without_text() {
        let path = write_test_png("pngmeta_test_notext.png", &[]);
        let chunks = read_text_chunks(&path).unwrap();
        assert!(chunks.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn keys_are_sorted() {
        let path = write_test_png(
            "pngmeta_test_sorted.png",
            &[("zebra", "z"), ("alpha", "a"), ("middle", "m")],
        );
        let chunks = read_text_chunks(&path).unwrap();
        let keys: Vec<&String> = chunks.keys().collect();
        assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
        std::fs::remove_file(&path).ok();
    }
}
