//! pngmetagrep-core: Extract tEXt metadata from PNG files.
//!
//! Primary use case: read VDSL recipe chunks (`"vdsl"`) embedded by the
//! VDSL image generation platform. Also works for ComfyUI's `"prompt"`
//! and `"workflow"` chunks or any arbitrary tEXt keyword.

pub use pngmeta::read_text_chunks;

use serde_json::Value;
use std::io;
use std::path::Path;

/// Extracted metadata from a single PNG file.
pub struct PngMeta {
    /// File path (as provided).
    pub path: String,
    /// Extracted chunks: (keyword, parsed JSON or raw string).
    pub chunks: Vec<(String, Value)>,
}

impl PngMeta {
    /// Convert to a JSON Value for serialization.
    ///
    /// - Single chunk whose value is a JSON object → merge `path` into it
    ///   (flat output: `{"path":"...", "_v":1, "seed":...}`).
    /// - Otherwise → nest by keyword
    ///   (`{"path":"...", "vdsl":{...}, "prompt":{...}}`).
    pub fn to_json_value(&self) -> Value {
        if self.chunks.len() == 1 {
            if let Value::Object(ref inner) = self.chunks[0].1 {
                let mut obj = serde_json::Map::with_capacity(inner.len() + 1);
                obj.insert("path".into(), Value::String(self.path.clone()));
                for (k, v) in inner {
                    obj.insert(k.clone(), v.clone());
                }
                return Value::Object(obj);
            }
        }

        let mut obj = serde_json::Map::with_capacity(self.chunks.len() + 1);
        obj.insert("path".into(), Value::String(self.path.clone()));
        for (key, val) in &self.chunks {
            obj.insert(key.clone(), val.clone());
        }
        Value::Object(obj)
    }
}

/// Extract tEXt chunks from a PNG file.
///
/// When `keys` is empty, **all** tEXt chunks are returned.
/// When `keys` is non-empty, only the specified keywords are extracted.
///
/// Each chunk value is parsed as JSON; if parsing fails, it is stored
/// as a JSON string. Files without any matching chunks return
/// `Ok(None)`.
pub fn extract(path: &Path, keys: &[String]) -> io::Result<Option<PngMeta>> {
    let text_chunks = pngmeta::read_text_chunks(path)?;

    let mut found = Vec::new();

    if keys.is_empty() {
        // BTreeMap yields entries in sorted order
        for (keyword, raw) in text_chunks {
            let value: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            found.push((keyword, value));
        }
    } else {
        for key in keys {
            if let Some(raw) = text_chunks.get(key.as_str()) {
                let value: Value =
                    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()));
                found.push((key.clone(), value));
            }
        }
    }

    if found.is_empty() {
        return Ok(None);
    }

    Ok(Some(PngMeta {
        path: path.display().to_string(),
        chunks: found,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_png(name: &str, chunks: &[(&str, &str)]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, pngmeta::test_util::make_test_png(chunks)).unwrap();
        path
    }

    #[test]
    fn extract_all_chunks_when_keys_empty() {
        let path = write_test_png(
            "pmg_ext_all.png",
            &[("prompt", "hello"), ("vdsl", r#"{"seed":1}"#)],
        );
        let result = extract(&path, &[]).unwrap().unwrap();
        assert_eq!(result.chunks.len(), 2);
        assert_eq!(result.chunks[0].0, "prompt");
        assert_eq!(result.chunks[1].0, "vdsl");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extract_specific_chunk() {
        let path = write_test_png(
            "pmg_ext_specific.png",
            &[("prompt", "hello"), ("vdsl", r#"{"seed":1}"#)],
        );
        let keys = vec!["vdsl".to_string()];
        let result = extract(&path, &keys).unwrap().unwrap();
        assert_eq!(result.chunks.len(), 1);
        assert_eq!(result.chunks[0].0, "vdsl");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extract_returns_none_for_missing_key() {
        let path = write_test_png("pmg_ext_miss.png", &[("prompt", "hello")]);
        let keys = vec!["nonexistent".to_string()];
        let result = extract(&path, &keys).unwrap();
        assert!(result.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extract_returns_none_for_no_text_chunks() {
        let path = write_test_png("pmg_ext_empty.png", &[]);
        let result = extract(&path, &[]).unwrap();
        assert!(result.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extract_parses_json_values() {
        let path = write_test_png("pmg_ext_json.png", &[("data", r#"{"key":"val"}"#)]);
        let result = extract(&path, &[]).unwrap().unwrap();
        assert!(result.chunks[0].1.is_object());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn extract_stores_non_json_as_string() {
        let path = write_test_png("pmg_ext_raw.png", &[("note", "plain text")]);
        let result = extract(&path, &[]).unwrap().unwrap();
        assert_eq!(result.chunks[0].1, Value::String("plain text".into()));
        std::fs::remove_file(&path).ok();
    }
}
