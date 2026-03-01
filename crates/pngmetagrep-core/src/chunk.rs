//! PNG tEXt chunk extraction — std only, no image decoding.
//!
//! PNG structure: 8-byte signature, then chunks of
//! length(4 BE) + type(4) + data(length) + crc(4).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Read big-endian u32 from a byte slice at the given offset.
fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Extract all tEXt chunks from a PNG file.
///
/// Returns keyword → text pairs. Only reads chunk headers and tEXt data;
/// image pixel data is never decoded.
pub fn read_text_chunks(path: &Path) -> io::Result<HashMap<String, String>> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a PNG file"));
    }

    let mut chunks = HashMap::new();
    let mut pos: usize = 8;

    while pos + 12 <= data.len() {
        let length = read_u32_be(&data, pos) as usize;
        let type_start = pos + 4;
        let type_end = pos + 8;
        let data_start = pos + 8;
        let data_end = data_start + length;

        if data_end > data.len() {
            break;
        }

        let chunk_type = &data[type_start..type_end];

        if chunk_type == b"tEXt" {
            let chunk_data = &data[data_start..data_end];
            if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                let keyword = String::from_utf8_lossy(&chunk_data[..null_pos]).into_owned();
                let text = String::from_utf8_lossy(&chunk_data[null_pos + 1..]).into_owned();
                chunks.insert(keyword, text);
            }
        } else if chunk_type == b"IEND" {
            break;
        }

        pos = data_end + 4; // skip crc(4)
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_png() {
        let dir = std::env::temp_dir();
        let path = dir.join("not_a_png.txt");
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
}
