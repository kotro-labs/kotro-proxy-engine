//! Hot-path benches matching production mcp-wrap:
//! compile once (tools/list), then validate_value + policy evaluate (tools/call).
//!
//! Also includes a queue-saturation load probe for the bounded schema pool.
//!
//!   cargo bench -p kotro-proxy --bench mcp_hot_path

use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use kotro_proxy::policy::{self, ToolCallContext, ToolClass};
use kotro_schema::{compile, ResourceLimits};

fn admitted_validate_plus_policy(c: &mut Criterion) {
    let engine = policy::PolicyEngine::compile(policy::presets::developer()).unwrap();
    let schema = serde_json::json!({
        "type": "object",
        "required": ["path"],
        "properties": {"path": {"type": "string"}}
    });
    let admitted = compile(&schema, &ResourceLimits::initial()).unwrap();
    let args = serde_json::json!({"path": "/tmp/notes.txt"});

    c.bench_function("admitted_validate_plus_policy", |b| {
        b.iter(|| {
            let result = admitted.validate_value(black_box(&args));
            let mut ctx = ToolCallContext {
                server: "files".into(),
                tool: "read_file".into(),
                class: ToolClass::ReadOnly,
                ..Default::default()
            };
            policy::extract_features(black_box(&args), &mut ctx);
            let decision = engine.evaluate(&ctx);
            black_box((result.ok, decision));
        })
    });
}

fn schema_pool_saturation(c: &mut Criterion) {
    let schema = serde_json::json!({
        "type": "object",
        "required": ["path"],
        "properties": {"path": {"type": "string"}}
    });
    let admitted = Arc::new(compile(&schema, &ResourceLimits::initial()).unwrap());
    let args = serde_json::json!({"path": "/tmp/notes.txt"});

    c.bench_function("schema_pool_concurrent_validate_8", |b| {
        b.iter(|| {
            let mut handles = Vec::new();
            for _ in 0..8 {
                let admitted = Arc::clone(&admitted);
                let args = args.clone();
                handles.push(thread::spawn(move || {
                    let t0 = Instant::now();
                    let ok = admitted.validate_value(&args).ok;
                    (ok, t0.elapsed())
                }));
            }
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            black_box(results);
        })
    });
}

criterion_group!(benches, admitted_validate_plus_policy, schema_pool_saturation);
criterion_main!(benches);
