//! Auto-detecting find with 3-level match strategy.
//!
//! Analyzes the search pattern at construction time and selects
//! the fastest matching path automatically:
//!
//! - **Level 1** (memmem): literal + case-sensitive
//! - **Level 2** (regex::bytes): regex without JSON structure chars
//! - **Level 3** (serde): regex containing `{`, `}`, or `"`

use regex::bytes::RegexBuilder as BytesRegexBuilder;
use regex::RegexBuilder;
use std::io;
use std::path::Path;

use crate::{extract, PngMeta};

/// Compiled match strategy — construct once, use many times.
pub enum Matcher {
    /// No filter: accept all files with tEXt chunks.
    None,
    /// Literal byte search (SIMD-accelerated memmem).
    BinContains(Vec<u8>),
    /// Regex on raw chunk bytes (no serde needed).
    BinRegex(regex::bytes::Regex),
    /// Regex requiring JSON serialization.
    JsonRegex(regex::Regex),
}

/// Options for constructing a [`Matcher`].
pub struct FindOptions<'a> {
    pub pattern: Option<&'a str>,
    pub ignore_case: bool,
}

/// True if the pattern contains no regex meta-characters.
fn is_literal(pattern: &str) -> bool {
    !pattern.chars().any(|c| {
        matches!(
            c,
            '.' | '^' | '$' | '*' | '+' | '?' | '{' | '}' | '[' | ']' | '(' | ')' | '|' | '\\'
        )
    })
}

/// True if the pattern references JSON structure characters that only
/// appear after serde serialization (braces, quotes).
fn has_json_structure_chars(pattern: &str) -> bool {
    pattern.chars().any(|c| matches!(c, '{' | '}' | '"'))
}

impl Matcher {
    /// Build a matcher from options. Auto-detects the optimal strategy.
    ///
    /// Returns `Err` if the pattern is an invalid regex.
    pub fn new(opts: &FindOptions<'_>) -> Result<Self, regex::Error> {
        let pat = match opts.pattern {
            Some(p) => p,
            None => return Ok(Matcher::None),
        };

        // Level 1: literal + case-sensitive → memmem
        if is_literal(pat) && !opts.ignore_case {
            return Ok(Matcher::BinContains(pat.as_bytes().to_vec()));
        }

        // Level 2: no JSON structure chars → regex::bytes
        if !has_json_structure_chars(pat) {
            let re = BytesRegexBuilder::new(pat)
                .case_insensitive(opts.ignore_case)
                .build()?;
            return Ok(Matcher::BinRegex(re));
        }

        // Level 3: full serde path
        let re = RegexBuilder::new(pat)
            .case_insensitive(opts.ignore_case)
            .build()?;
        Ok(Matcher::JsonRegex(re))
    }

    /// Test if any tEXt chunk in the file matches (binary pre-filter).
    ///
    /// Returns `Some(true/false)` for Level 1/2/None,
    /// `Option::None` for Level 3 (needs serde path).
    pub fn bin_matches(&self, path: &Path) -> Option<bool> {
        match self {
            Matcher::None => Option::None, // must go through extract to verify chunks exist
            Matcher::BinContains(needle) => pngmeta::contains_in_text_chunks(path, needle).ok(),
            Matcher::BinRegex(re) => pngmeta::scan_text_chunks(path, |data| re.is_match(data)).ok(),
            Matcher::JsonRegex(_) => Option::None,
        }
    }

    /// Full find: extract + filter in one call.
    ///
    /// Uses the fastest available path automatically:
    /// - Level 1/2: binary pre-filter → skip extract on miss
    /// - Level 3: extract → serde → regex
    ///
    /// Returns `Ok(None)` if the file has no matching chunks.
    pub fn find(&self, path: &Path, keys: &[String]) -> io::Result<Option<PngMeta>> {
        // Binary pre-filter (Level 1 & 2)
        if let Some(matched) = self.bin_matches(path) {
            if !matched {
                return Ok(None);
            }
        }

        let meta = match extract(path, keys)? {
            Some(m) => m,
            None => return Ok(None),
        };

        // Level 3: serde + regex
        if let Matcher::JsonRegex(ref re) = *self {
            let json = serde_json::to_string(&meta.to_json_value())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            if !re.is_match(&json) {
                return Ok(None);
            }
        }

        Ok(Some(meta))
    }

    /// Check if a file matches without extracting full metadata.
    ///
    /// Faster than [`find`](Self::find) when you only need a boolean
    /// (e.g. `-l` files-only mode).
    pub fn matches(&self, path: &Path, keys: &[String]) -> io::Result<bool> {
        // Binary pre-filter (Level 1 & 2)
        if let Some(matched) = self.bin_matches(path) {
            return Ok(matched);
        }

        // Level 3: need extract + serde
        let meta = match extract(path, keys)? {
            Some(m) => m,
            None => return Ok(false),
        };

        if let Matcher::JsonRegex(ref re) = *self {
            let json = serde_json::to_string(&meta.to_json_value())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            return Ok(re.is_match(&json));
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_png(name: &str, chunks: &[(&str, &str)]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, pngmeta::test_util::make_test_png(chunks)).unwrap();
        path
    }

    fn opts(pattern: Option<&str>, ignore_case: bool) -> FindOptions<'_> {
        FindOptions {
            pattern,
            ignore_case,
        }
    }

    // --- Matcher::new auto-detection ---

    #[test]
    fn auto_none() {
        let m = Matcher::new(&opts(None, false)).unwrap();
        assert!(matches!(m, Matcher::None));
    }

    #[test]
    fn auto_literal() {
        let m = Matcher::new(&opts(Some("seed"), false)).unwrap();
        assert!(matches!(m, Matcher::BinContains(_)));
    }

    #[test]
    fn auto_literal_ignore_case_upgrades_to_bin_regex() {
        let m = Matcher::new(&opts(Some("seed"), true)).unwrap();
        assert!(matches!(m, Matcher::BinRegex(_)));
    }

    #[test]
    fn auto_regex_no_json_chars() {
        let m = Matcher::new(&opts(Some("seed|model"), false)).unwrap();
        assert!(matches!(m, Matcher::BinRegex(_)));
    }

    #[test]
    fn auto_json_regex() {
        let m = Matcher::new(&opts(Some(r#"\{"seed""#), false)).unwrap();
        assert!(matches!(m, Matcher::JsonRegex(_)));
    }

    #[test]
    fn auto_invalid_regex() {
        let result = Matcher::new(&opts(Some("[invalid"), false));
        assert!(result.is_err());
    }

    // --- find ---

    #[test]
    fn find_literal_hit() {
        let path = write_test_png("find_lit_hit.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("seed"), false)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_some());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn find_literal_miss() {
        let path = write_test_png("find_lit_miss.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("nonexistent"), false)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn find_regex_hit() {
        let path = write_test_png("find_re_hit.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("seed|model"), false)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_some());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn find_regex_miss() {
        let path = write_test_png("find_re_miss.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("xyz|abc"), false)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn find_no_filter() {
        let path = write_test_png("find_none.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(None, false)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_some());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn find_ignore_case() {
        let path = write_test_png("find_icase.png", &[("vdsl", r#"{"Seed":42}"#)]);
        let m = Matcher::new(&opts(Some("seed"), true)).unwrap();
        let result = m.find(&path, &[]).unwrap();
        assert!(result.is_some());
        std::fs::remove_file(&path).ok();
    }

    // --- matches ---

    #[test]
    fn matches_fast_path() {
        let path = write_test_png("matches_fast.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("seed"), false)).unwrap();
        assert!(m.matches(&path, &[]).unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn matches_fast_path_miss() {
        let path = write_test_png("matches_fast_miss.png", &[("vdsl", r#"{"seed":42}"#)]);
        let m = Matcher::new(&opts(Some("nope"), false)).unwrap();
        assert!(!m.matches(&path, &[]).unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn matches_empty_png() {
        let path = write_test_png("matches_empty.png", &[]);
        let m = Matcher::new(&opts(None, false)).unwrap();
        assert!(!m.matches(&path, &[]).unwrap());
        std::fs::remove_file(&path).ok();
    }
}
