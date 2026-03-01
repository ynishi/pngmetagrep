//! pngmetagrep-core: Extract tEXt metadata from PNG files.
//!
//! Primary use case: read VDSL recipe chunks (`"vdsl"`) embedded by the
//! VDSL image generation platform. Also works for ComfyUI's `"prompt"`
//! and `"workflow"` chunks or any arbitrary tEXt keyword.

mod chunk;

pub use chunk::read_text_chunks;

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

/// Extract specified tEXt chunks from a PNG file.
///
/// Each chunk value is parsed as JSON; if parsing fails, it is stored
/// as a JSON string. Files without any of the requested chunks return
/// `Ok(None)`.
pub fn extract(path: &Path, keys: &[String]) -> io::Result<Option<PngMeta>> {
    let text_chunks = read_text_chunks(path)?;

    let mut found = Vec::new();
    for key in keys {
        if let Some(raw) = text_chunks.get(key.as_str()) {
            let value: Value =
                serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()));
            found.push((key.clone(), value));
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
