//! Latency benchmark for the local semantic-cache embedding step
//! (`SemanticEncoder::embed`) — the P2 item in docs/roadmap/next-steps.md:
//! "Benchmark embedding latency overhead ... must stay low enough that it
//! doesn't erode the savings it creates."
//!
//! `embed()` runs synchronously in the request hot path (see
//! `router/handlers.rs`), so this measures exactly the added latency every
//! cached-eligible request pays before the exact-match/vector cache lookup
//! even happens.
//!
//! Run in release mode — debug-mode CPU inference is not representative:
//!
//!   cd rust && cargo run --release --example bench_embedding
//!
//! First run downloads ~90MB into the HF cache (see cache/vector.rs docs);
//! that one-time cost is measured and reported separately from per-request
//! embedding latency, since it happens once at proxy startup, not per request.

use std::time::Instant;

use kotro_proxy::cache::vector::SemanticEncoder;

/// Representative prompt shapes for a coding-agent turn: a short one-liner,
/// a medium question with an inline code snippet, and a long turn carrying
/// a chunk of file content — the same range of sizes real agent traffic
/// produces, per docs/security/THREAT-MODEL.md's eval fixture description.
const PROMPTS: &[(&str, &str)] = &[
    ("short (~10 words)", "Fix the typo in the print statement below."),
    (
        "medium (~60 words, inline code)",
        "Can you review this function and tell me if there's a bug in the \
         error handling? ```rust\nfn parse_config(path: &str) -> Result<Config, Error> {\n    \
         let contents = std::fs::read_to_string(path)?;\n    let config: Config = \
         serde_json::from_str(&contents)?;\n    Ok(config)\n}\n``` It seems to panic \
         sometimes when the file is missing.",
    ),
    (
        "long (~300 words, file-content turn)",
        include_str!("bench_embedding_long_prompt.txt"),
    ),
];

const WARMUP_ITERS: usize = 5;
const TIMED_ITERS: usize = 50;

fn percentile(sorted_ms: &[f64], pct: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ms.len() as f64 - 1.0) * pct).round() as usize;
    sorted_ms[idx.min(sorted_ms.len() - 1)]
}

fn main() {
    println!("Loading semantic embedding model (one-time, includes download if not cached)...");
    let load_start = Instant::now();
    let encoder = SemanticEncoder::new(true);
    let load_elapsed = load_start.elapsed();
    println!("Model load time: {:.1} ms\n", load_elapsed.as_secs_f64() * 1000.0);

    // Confirm the model actually loaded rather than silently degrading to
    // disabled -- an empty-string probe returns None either way, so probe
    // with real text instead.
    if encoder.embed(PROMPTS[0].1).is_none() {
        eprintln!(
            "Semantic encoder is disabled or failed to load in this environment \
             (offline? no HF cache?) -- nothing to benchmark. See cache/vector.rs \
             SemanticEncoder::new() for what a load failure looks like in logs."
        );
        std::process::exit(1);
    }

    println!(
        "Benchmarking embed() latency: {WARMUP_ITERS} warmup + {TIMED_ITERS} timed \
         iterations per prompt shape.\n"
    );

    for (label, prompt) in PROMPTS {
        for _ in 0..WARMUP_ITERS {
            let _ = encoder.embed(prompt);
        }

        let mut samples_ms: Vec<f64> = Vec::with_capacity(TIMED_ITERS);
        for _ in 0..TIMED_ITERS {
            let start = Instant::now();
            let result = encoder.embed(prompt);
            let elapsed = start.elapsed();
            assert!(result.is_some(), "embed() unexpectedly returned None mid-benchmark");
            samples_ms.push(elapsed.as_secs_f64() * 1000.0);
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mean: f64 = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;
        let p50 = percentile(&samples_ms, 0.50);
        let p95 = percentile(&samples_ms, 0.95);
        let p99 = percentile(&samples_ms, 0.99);
        let min = samples_ms.first().copied().unwrap_or(0.0);
        let max = samples_ms.last().copied().unwrap_or(0.0);
        let token_count = prompt.split_whitespace().count();

        println!(
            "{label} (~{token_count} whitespace-split words):\n  \
             mean={mean:.2}ms  p50={p50:.2}ms  p95={p95:.2}ms  p99={p99:.2}ms  \
             min={min:.2}ms  max={max:.2}ms\n"
        );
    }

    println!(
        "Compare p50/p95 above against the exact-match cache-hit path latency \
         (KOTRO_CACHE_HIT_DELAY_MS, default 2ms) and against typical upstream \
         provider round-trip time (hundreds of ms) to judge overhead. Paste \
         these numbers back for docs/roadmap/next-steps.md and the eval suite."
    );
}
