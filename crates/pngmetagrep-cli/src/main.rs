use clap::Parser;
use rayon::prelude::*;
use regex::bytes::RegexBuilder as BytesRegexBuilder;
use regex::RegexBuilder;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "pngmetagrep", about = "PNG tEXt metadata → NDJSON", version)]
struct Cli {
    /// Directories or files to scan (default: current directory).
    paths: Vec<PathBuf>,

    /// tEXt chunk keyword to extract (repeatable, default: all chunks).
    #[arg(long = "chunk")]
    chunks: Vec<String>,

    /// Regex filter on serialized JSON (only matching files are output).
    #[arg(short = 'e')]
    pattern: Option<String>,

    /// Case-insensitive matching for -e.
    #[arg(short = 'i')]
    ignore_case: bool,

    /// Print matching file paths only (no JSON).
    #[arg(short = 'l')]
    files_only: bool,

    /// Number of parallel threads (default: CPU count).
    #[arg(short = 'j')]
    threads: Option<usize>,
}

/// Match strategy — dispatched by pattern content.
enum MatchStrategy {
    /// No filter pattern.
    None,
    /// Literal string, case-sensitive → memmem fast path.
    BinContains(Vec<u8>),
    /// Regex without JSON structure chars → regex::bytes on raw chunks.
    BinRegex(regex::bytes::Regex),
    /// Regex with JSON structure chars → serde serialization required.
    JsonRegex(regex::Regex),
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

fn build_strategy(cli: &Cli) -> MatchStrategy {
    let pat = match cli.pattern.as_deref() {
        Some(p) => p,
        None => return MatchStrategy::None,
    };

    // Level 1: literal + case-sensitive → memmem
    if is_literal(pat) && !cli.ignore_case {
        return MatchStrategy::BinContains(pat.as_bytes().to_vec());
    }

    // Level 2: no JSON structure chars → regex::bytes on raw chunk data
    if !has_json_structure_chars(pat) {
        let re = BytesRegexBuilder::new(pat)
            .case_insensitive(cli.ignore_case)
            .build()
            .unwrap_or_else(|e| {
                eprintln!("pngmetagrep: invalid regex: {e}");
                std::process::exit(1);
            });
        return MatchStrategy::BinRegex(re);
    }

    // Level 3: full serde path
    let re = RegexBuilder::new(pat)
        .case_insensitive(cli.ignore_case)
        .build()
        .unwrap_or_else(|e| {
            eprintln!("pngmetagrep: invalid regex: {e}");
            std::process::exit(1);
        });
    MatchStrategy::JsonRegex(re)
}

/// Binary-level pre-filter: does any tEXt chunk match the pattern?
fn bin_matches(path: &Path, strategy: &MatchStrategy) -> Option<bool> {
    match strategy {
        MatchStrategy::None => Some(true),
        MatchStrategy::BinContains(needle) => pngmeta::contains_in_text_chunks(path, needle).ok(),
        MatchStrategy::BinRegex(re) => {
            pngmeta::scan_text_chunks(path, |data| re.is_match(data)).ok()
        }
        MatchStrategy::JsonRegex(_) => Option::None, // needs serde path
    }
}

fn collect_pngs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            files.push(root.clone());
            continue;
        }
        files.extend(
            WalkDir::new(root)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().is_file()
                        && e.path()
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                })
                .map(|e| e.into_path()),
        );
    }
    files
}

fn main() {
    let cli = Cli::parse();

    let keys: Vec<String> = cli.chunks.clone();

    if let Some(n) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    let strategy = build_strategy(&cli);

    let paths = if cli.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        cli.paths.clone()
    };

    let files = collect_pngs(&paths);

    let results: Vec<String> = files
        .par_iter()
        .filter_map(|path| {
            // Fast path: binary pre-filter (Level 1 & 2)
            if let Some(matched) = bin_matches(path, &strategy) {
                if !matched {
                    return None;
                }

                // files_only + bin match → no need for extract/serde at all
                if cli.files_only {
                    return Some(path.display().to_string());
                }
            }

            // Need extract for JSON output or JsonRegex matching
            let meta = match pngmetagrep_core::extract(path, &keys) {
                Ok(Some(m)) => m,
                Ok(None) => return None,
                Err(e) => {
                    eprintln!("pngmetagrep: {}: {e}", path.display());
                    return None;
                }
            };

            // Level 3 (JsonRegex): serde + regex match
            if let MatchStrategy::JsonRegex(ref re) = strategy {
                let json = serde_json::to_string(&meta.to_json_value()).ok()?;
                if !re.is_match(&json) {
                    return None;
                }
                if cli.files_only {
                    return Some(meta.path);
                }
                return Some(json);
            }

            // Level 1 & 2 already matched — just produce JSON output
            let json = serde_json::to_string(&meta.to_json_value()).ok()?;
            Some(json)
        })
        .collect();

    let stdout = io::stdout().lock();
    let mut writer = BufWriter::new(stdout);
    for line in &results {
        let _ = writeln!(writer, "{line}");
    }
}
