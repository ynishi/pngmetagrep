//! PNG tEXt chunk extraction — streaming, no image decoding.
//!
//! PNG structure: 8-byte signature, then chunks of
//! length(4 BE) + type(4) + data(length) + crc(4).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use memchr::memmem;

use crate::PNG_SIGNATURE;

/// Scan tEXt chunks with a caller-supplied predicate on raw bytes.
///
/// Iterates over tEXt chunks in the PNG file, passing each chunk's raw
/// data (keyword + null + text) to `predicate`. Returns `Ok(true)` as
/// soon as any call returns `true` (early exit).
///
/// This is the low-level building block for binary-level searches.
/// No UTF-8 decoding, JSON parsing, or collection construction occurs.
pub fn scan_text_chunks<F>(path: &Path, mut predicate: F) -> io::Result<bool>
where
    F: FnMut(&[u8]) -> bool,
{
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut sig = [0u8; 8];
    reader.read_exact(&mut sig)?;
    if sig != PNG_SIGNATURE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a PNG file"));
    }

    let mut header = [0u8; 8];

    while reader.read_exact(&mut header).is_ok() {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);

        if &header[4..8] == b"tEXt" {
            let mut data = vec![0u8; length as usize];
            reader.read_exact(&mut data)?;
            reader.seek(SeekFrom::Current(4))?; // skip CRC

            if predicate(&data) {
                return Ok(true);
            }
        } else if &header[4..8] == b"IEND" {
            break;
        } else {
            reader.seek(SeekFrom::Current(i64::from(length) + 4))?;
        }
    }

    Ok(false)
}

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

/// Search tEXt chunk data for a byte pattern without decoding.
///
/// SIMD-accelerated (`memmem`) search over each chunk's raw bytes.
/// Returns `true` on first match (early exit). Wrapper around
/// [`scan_text_chunks`].
pub fn contains_in_text_chunks(path: &Path, needle: &[u8]) -> io::Result<bool> {
    let finder = memmem::Finder::new(needle);
    scan_text_chunks(path, |data| finder.find(data).is_some())
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

    // --- read_text_chunks tests ---

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

    // --- scan_text_chunks tests ---

    #[test]
    fn scan_returns_true_on_predicate_match() {
        let path = write_test_png("pngmeta_scan_match.png", &[("key", "value")]);
        let result =
            scan_text_chunks(&path, |data| data.windows(5).any(|w| w == b"value")).unwrap();
        assert!(result);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_returns_false_when_no_match() {
        let path = write_test_png("pngmeta_scan_nomatch.png", &[("key", "value")]);
        let result =
            scan_text_chunks(&path, |data| data.windows(6).any(|w| w == b"absent")).unwrap();
        assert!(!result);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_early_exits() {
        let path = write_test_png(
            "pngmeta_scan_early.png",
            &[("a", "hit"), ("b", "hit"), ("c", "miss")],
        );
        let mut call_count = 0usize;
        let result = scan_text_chunks(&path, |data| {
            call_count += 1;
            data.windows(3).any(|w| w == b"hit")
        })
        .unwrap();
        assert!(result);
        assert_eq!(call_count, 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn scan_rejects_non_png() {
        let path = std::env::temp_dir().join("pngmeta_scan_bad.txt");
        std::fs::write(&path, b"not png").unwrap();
        assert!(scan_text_chunks(&path, |_| true).is_err());
        std::fs::remove_file(&path).ok();
    }

    // --- contains_in_text_chunks tests ---

    #[test]
    fn contains_finds_in_text_value() {
        let path = write_test_png("pngmeta_contains_val.png", &[("vdsl", r#"{"seed":42}"#)]);
        assert!(contains_in_text_chunks(&path, b"seed").unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_finds_in_keyword() {
        let path = write_test_png("pngmeta_contains_kw.png", &[("workflow", "data")]);
        assert!(contains_in_text_chunks(&path, b"workflow").unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_returns_false_when_absent() {
        let path = write_test_png("pngmeta_contains_miss.png", &[("vdsl", r#"{"seed":1}"#)]);
        assert!(!contains_in_text_chunks(&path, b"nonexistent").unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_returns_false_for_empty_png() {
        let path = write_test_png("pngmeta_contains_empty.png", &[]);
        assert!(!contains_in_text_chunks(&path, b"anything").unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_early_exits_on_first_match() {
        let path = write_test_png(
            "pngmeta_contains_early.png",
            &[("a", "needle"), ("b", "needle"), ("c", "other")],
        );
        assert!(contains_in_text_chunks(&path, b"needle").unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn contains_rejects_non_png() {
        let path = std::env::temp_dir().join("pngmeta_contains_bad.txt");
        std::fs::write(&path, b"not png").unwrap();
        assert!(contains_in_text_chunks(&path, b"x").is_err());
        std::fs::remove_file(&path).ok();
    }
}
