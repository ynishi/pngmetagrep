use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pngmeta::test_util::make_test_png;
use std::path::PathBuf;

/// Generate a realistic-ish PNG with a large JSON tEXt chunk.
fn setup_test_file(name: &str, chunk_size: usize) -> PathBuf {
    let json_value = serde_json::json!({
        "_v": 1,
        "seed": 42,
        "model": "sdxl-turbo",
        "prompt": "a".repeat(chunk_size),
        "negative_prompt": "blurry, low quality",
        "steps": 20,
        "cfg_scale": 7.5,
        "sampler": "euler_a",
        "width": 1024,
        "height": 1024,
    });
    let json_str = serde_json::to_string(&json_value).unwrap();
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, make_test_png(&[("vdsl", &json_str)])).unwrap();
    path
}

fn bench_match_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("match_strategy");

    for &size in &[100, 1_000, 10_000] {
        let label = format!("{size}B");
        let path = setup_test_file(&format!("bench_match_{size}.png"), size);

        // Level 1: memmem (BinContains)
        group.bench_with_input(BenchmarkId::new("L1_memmem", &label), &path, |b, path| {
            b.iter(|| {
                black_box(pngmeta::contains_in_text_chunks(path, b"seed").unwrap());
            });
        });

        // Level 2: regex::bytes (BinRegex)
        let bin_re = regex::bytes::Regex::new("seed|model").unwrap();
        group.bench_with_input(
            BenchmarkId::new("L2_bin_regex", &label),
            &path,
            |b, path| {
                b.iter(|| {
                    black_box(
                        pngmeta::scan_text_chunks(path, |data| bin_re.is_match(data)).unwrap(),
                    );
                });
            },
        );

        // Level 3a: read_text_chunks + String regex (no serde)
        let str_re = regex::Regex::new("seed|model").unwrap();
        group.bench_with_input(
            BenchmarkId::new("L3a_read_str_regex", &label),
            &path,
            |b, path| {
                b.iter(|| {
                    let chunks = pngmeta::read_text_chunks(path).unwrap();
                    let matched = chunks.values().any(|v| str_re.is_match(v));
                    black_box(matched);
                });
            },
        );

        // Level 3b: read_text_chunks + serde serialize + String regex (current path)
        group.bench_with_input(
            BenchmarkId::new("L3b_read_serde_regex", &label),
            &path,
            |b, path| {
                b.iter(|| {
                    let chunks = pngmeta::read_text_chunks(path).unwrap();
                    let mut obj = serde_json::Map::new();
                    for (k, v) in &chunks {
                        let val: serde_json::Value =
                            serde_json::from_str(v).unwrap_or(serde_json::Value::String(v.clone()));
                        obj.insert(k.clone(), val);
                    }
                    let json = serde_json::to_string(&obj).unwrap();
                    let matched = str_re.is_match(&json);
                    black_box(matched);
                });
            },
        );

        std::fs::remove_file(&path).ok();
    }

    group.finish();
}

fn bench_miss_fast_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("miss_fast_path");

    let path = setup_test_file("bench_miss.png", 5_000);

    // Miss case: needle not present — this is where fast path matters most
    group.bench_function("L1_memmem_miss", |b| {
        b.iter(|| {
            black_box(pngmeta::contains_in_text_chunks(&path, b"NONEXISTENT_PATTERN").unwrap());
        });
    });

    let bin_re = regex::bytes::Regex::new("NONEXISTENT_PATTERN").unwrap();
    group.bench_function("L2_bin_regex_miss", |b| {
        b.iter(|| {
            black_box(pngmeta::scan_text_chunks(&path, |data| bin_re.is_match(data)).unwrap());
        });
    });

    let str_re = regex::Regex::new("NONEXISTENT_PATTERN").unwrap();
    group.bench_function("L3b_serde_regex_miss", |b| {
        b.iter(|| {
            let chunks = pngmeta::read_text_chunks(&path).unwrap();
            let mut obj = serde_json::Map::new();
            for (k, v) in &chunks {
                let val: serde_json::Value =
                    serde_json::from_str(v).unwrap_or(serde_json::Value::String(v.clone()));
                obj.insert(k.clone(), val);
            }
            let json = serde_json::to_string(&obj).unwrap();
            black_box(str_re.is_match(&json));
        });
    });

    std::fs::remove_file(&path).ok();
    group.finish();
}

criterion_group!(benches, bench_match_strategies, bench_miss_fast_path);
criterion_main!(benches);
