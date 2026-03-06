//! Test utilities for building minimal valid PNG files.

use crate::PNG_SIGNATURE;

/// Build a minimal valid PNG with the given tEXt chunks (test helper).
pub fn make_test_png(text_chunks: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&PNG_SIGNATURE);

    // IHDR: 1×1 8-bit grayscale
    let ihdr: [u8; 13] = [0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0];
    buf.extend_from_slice(&13u32.to_be_bytes());
    buf.extend_from_slice(b"IHDR");
    buf.extend_from_slice(&ihdr);
    buf.extend_from_slice(&[0; 4]); // CRC (not validated by reader)

    for (kw, txt) in text_chunks {
        let len = kw.len() + 1 + txt.len();
        buf.extend_from_slice(&(len as u32).to_be_bytes());
        buf.extend_from_slice(b"tEXt");
        buf.extend_from_slice(kw.as_bytes());
        buf.push(0);
        buf.extend_from_slice(txt.as_bytes());
        buf.extend_from_slice(&[0; 4]); // CRC
    }

    // IEND
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(b"IEND");
    buf.extend_from_slice(&[0; 4]); // CRC

    buf
}
