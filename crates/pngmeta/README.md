# pngmeta

Low-level PNG tEXt chunk reader/writer. No image decoding — operates directly on binary PNG structure using only `std` + `memchr`.

## Features

- **Read** — extract all tEXt chunks as `BTreeMap<String, String>`
- **Scan** — streaming predicate search with early exit (no allocation per chunk)
- **Contains** — SIMD-accelerated (`memchr::memmem`) binary pattern search
- **Write** — insert tEXt chunks before IEND with correct CRC-32

## Usage

```rust
use std::path::Path;
use pngmeta::{read_text_chunks, contains_in_text_chunks, write_text_chunk};

// Read all tEXt chunks
let chunks = read_text_chunks(Path::new("image.png"))?;
for (key, value) in &chunks {
    println!("{key}: {value}");
}

// Fast binary search (no UTF-8 decoding)
if contains_in_text_chunks(Path::new("image.png"), b"seed")? {
    println!("found");
}

// Write a tEXt chunk
write_text_chunk(Path::new("image.png"), "prompt", "a cat")?;
```

## Feature Flags

| Flag | Description |
|------|-------------|
| `test-util` | Expose `test_util::make_test_png` helper for downstream test code |

## License

MIT
