use clap::Parser;
use pngmetagrep_core::{FindOptions, Matcher};
use rayon::prelude::*;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
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

    /// Stop after N matches.
    #[arg(short = 'n', long = "limit")]
    limit: Option<usize>,

    /// Print match count only.
    #[arg(short = 'c', long = "count")]
    count: bool,

    /// Number of parallel threads (default: CPU count).
    #[arg(short = 'j')]
    threads: Option<usize>,
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
    // Reset SIGPIPE to default so BrokenPipe is delivered properly
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();

    let keys: Vec<String> = cli.chunks.clone();
    let files_only = cli.files_only;
    let count_only = cli.count;
    let limit = cli.limit;

    if let Some(n) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    let matcher = Matcher::new(&FindOptions {
        pattern: cli.pattern.as_deref(),
        ignore_case: cli.ignore_case,
    })
    .unwrap_or_else(|e| {
        eprintln!("pngmetagrep: invalid regex: {e}");
        std::process::exit(1);
    });

    let paths = if cli.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        cli.paths.clone()
    };

    let files = collect_pngs(&paths);

    // Shared stop flag: set when limit reached or pipe broken
    let stop = AtomicBool::new(false);
    let match_count = AtomicUsize::new(0);

    // Bounded channel: backpressure when consumer is slow
    let (tx, rx) = mpsc::sync_channel::<String>(64);

    // Consumer thread: write to stdout
    let consumer = std::thread::spawn(move || {
        let stdout = io::stdout().lock();
        let mut writer = BufWriter::new(stdout);

        if count_only {
            let mut n = 0usize;
            for _line in rx {
                n += 1;
            }
            let _ = writeln!(writer, "{n}");
            return;
        }

        for line in rx {
            if writeln!(writer, "{line}").is_err() {
                break;
            }
        }
    });

    // Producer: parallel scan, streaming results via channel
    files.par_iter().for_each(|path| {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        let line = if files_only {
            match matcher.matches(path, &keys) {
                Ok(true) => path.display().to_string(),
                Ok(false) => return,
                Err(e) => {
                    eprintln!("pngmetagrep: {}: {e}", path.display());
                    return;
                }
            }
        } else {
            match matcher.find(path, &keys) {
                Ok(Some(meta)) => match serde_json::to_string(&meta.to_json_value()) {
                    Ok(json) => json,
                    Err(_) => return,
                },
                Ok(None) => return,
                Err(e) => {
                    eprintln!("pngmetagrep: {}: {e}", path.display());
                    return;
                }
            }
        };

        // Check limit
        if let Some(max) = limit {
            let prev = match_count.fetch_add(1, Ordering::Relaxed);
            if prev >= max {
                stop.store(true, Ordering::Relaxed);
                return;
            }
        }

        // Send may fail if consumer has stopped (BrokenPipe)
        if tx.send(line).is_err() {
            stop.store(true, Ordering::Relaxed);
        }
    });

    // Drop sender so consumer thread can finish
    drop(tx);
    let _ = consumer.join();
}
