use clap::Parser;
use rayon::prelude::*;
use regex::RegexBuilder;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "pngmetagrep", about = "PNG tEXt metadata → NDJSON", version)]
struct Cli {
    /// Directory or file to scan.
    path: PathBuf,

    /// tEXt chunk keyword to extract (repeatable, default: vdsl).
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

fn collect_pngs(root: &PathBuf) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.clone()];
    }

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
        .map(|e| e.into_path())
        .collect()
}

fn main() {
    let cli = Cli::parse();

    let keys: Vec<String> = if cli.chunks.is_empty() {
        vec!["vdsl".into()]
    } else {
        cli.chunks
    };

    if let Some(n) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    let re = cli.pattern.as_ref().map(|pat| {
        RegexBuilder::new(pat)
            .case_insensitive(cli.ignore_case)
            .build()
            .unwrap_or_else(|e| {
                eprintln!("pngmetagrep: invalid regex: {e}");
                std::process::exit(1);
            })
    });

    let files = collect_pngs(&cli.path);

    let results: Vec<String> = files
        .par_iter()
        .filter_map(|path| {
            let meta = pngmetagrep_core::extract(path, &keys).ok()??;

            if cli.files_only {
                if let Some(ref re) = re {
                    let json = serde_json::to_string(&meta.to_json_value()).ok()?;
                    if !re.is_match(&json) {
                        return None;
                    }
                }
                return Some(meta.path);
            }

            let json = serde_json::to_string(&meta.to_json_value()).ok()?;

            if let Some(ref re) = re {
                if !re.is_match(&json) {
                    return None;
                }
            }

            Some(json)
        })
        .collect();

    let stdout = io::stdout().lock();
    let mut writer = BufWriter::new(stdout);
    for line in &results {
        let _ = writeln!(writer, "{line}");
    }
}
