//! pngmeta: Read and write PNG tEXt metadata chunks.
//!
//! Low-level library for PNG tEXt chunk I/O without image decoding.
//! Operates directly on the binary PNG structure using only std.

mod read;
mod write;

#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use read::{contains_in_text_chunks, read_text_chunks, scan_text_chunks};
pub use write::write_text_chunk;

/// PNG file signature (8 bytes).
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
