//! Process-local schema pool telemetry (C5 worker pool).
//!
//! Counters are always updated. An optional hook lets the proxy / mcp-wrap
//! process mirror events into Prometheus.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Why argument validation returned `validation_unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableCause {
    QueueFull,
    Deadline,
    WorkerPanic,
    PoolInit,
}

impl UnavailableCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueueFull => "queue_full",
            Self::Deadline => "deadline",
            Self::WorkerPanic => "worker_panic",
            Self::PoolInit => "pool_init",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SchemaEvent {
    Unavailable(UnavailableCause),
    QueueSaturated,
    ValidationLatency(Duration),
    CompileLatency(Duration),
}

#[derive(Debug, Clone, Default)]
pub struct SchemaTelemetrySnapshot {
    pub unavailable_queue_full: u64,
    pub unavailable_deadline: u64,
    pub unavailable_worker_panic: u64,
    pub unavailable_pool_init: u64,
    pub queue_saturated: u64,
    pub validation_samples: u64,
    pub validation_latency_ns_sum: u64,
    pub compile_samples: u64,
    pub compile_latency_ns_sum: u64,
}

static UNAVAILABLE_QUEUE_FULL: AtomicU64 = AtomicU64::new(0);
static UNAVAILABLE_DEADLINE: AtomicU64 = AtomicU64::new(0);
static UNAVAILABLE_WORKER_PANIC: AtomicU64 = AtomicU64::new(0);
static UNAVAILABLE_POOL_INIT: AtomicU64 = AtomicU64::new(0);
static QUEUE_SATURATED: AtomicU64 = AtomicU64::new(0);
static VALIDATION_SAMPLES: AtomicU64 = AtomicU64::new(0);
static VALIDATION_LATENCY_NS: AtomicU64 = AtomicU64::new(0);
static COMPILE_SAMPLES: AtomicU64 = AtomicU64::new(0);
static COMPILE_LATENCY_NS: AtomicU64 = AtomicU64::new(0);

static HOOK: OnceLock<Mutex<Option<Box<dyn Fn(SchemaEvent) + Send + Sync>>>> = OnceLock::new();

fn hook_slot() -> &'static Mutex<Option<Box<dyn Fn(SchemaEvent) + Send + Sync>>> {
    HOOK.get_or_init(|| Mutex::new(None))
}

/// Install (or replace) a process-wide telemetry hook. Used by the proxy to
/// mirror events into Prometheus counters/histograms.
pub fn install_hook<F>(hook: F)
where
    F: Fn(SchemaEvent) + Send + Sync + 'static,
{
    if let Ok(mut guard) = hook_slot().lock() {
        *guard = Some(Box::new(hook));
    }
}

fn emit(event: SchemaEvent) {
    if let Ok(guard) = hook_slot().lock() {
        if let Some(hook) = guard.as_ref() {
            hook(event);
        }
    }
}

pub fn record_unavailable(cause: UnavailableCause) {
    match cause {
        UnavailableCause::QueueFull => {
            UNAVAILABLE_QUEUE_FULL.fetch_add(1, Ordering::Relaxed);
            QUEUE_SATURATED.fetch_add(1, Ordering::Relaxed);
            emit(SchemaEvent::QueueSaturated);
        }
        UnavailableCause::Deadline => {
            UNAVAILABLE_DEADLINE.fetch_add(1, Ordering::Relaxed);
        }
        UnavailableCause::WorkerPanic => {
            UNAVAILABLE_WORKER_PANIC.fetch_add(1, Ordering::Relaxed);
        }
        UnavailableCause::PoolInit => {
            UNAVAILABLE_POOL_INIT.fetch_add(1, Ordering::Relaxed);
        }
    }
    emit(SchemaEvent::Unavailable(cause));
}

pub fn record_validation_latency(d: Duration) {
    VALIDATION_SAMPLES.fetch_add(1, Ordering::Relaxed);
    VALIDATION_LATENCY_NS.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    emit(SchemaEvent::ValidationLatency(d));
}

pub fn record_compile_latency(d: Duration) {
    COMPILE_SAMPLES.fetch_add(1, Ordering::Relaxed);
    COMPILE_LATENCY_NS.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    emit(SchemaEvent::CompileLatency(d));
}

pub fn snapshot() -> SchemaTelemetrySnapshot {
    SchemaTelemetrySnapshot {
        unavailable_queue_full: UNAVAILABLE_QUEUE_FULL.load(Ordering::Relaxed),
        unavailable_deadline: UNAVAILABLE_DEADLINE.load(Ordering::Relaxed),
        unavailable_worker_panic: UNAVAILABLE_WORKER_PANIC.load(Ordering::Relaxed),
        unavailable_pool_init: UNAVAILABLE_POOL_INIT.load(Ordering::Relaxed),
        queue_saturated: QUEUE_SATURATED.load(Ordering::Relaxed),
        validation_samples: VALIDATION_SAMPLES.load(Ordering::Relaxed),
        validation_latency_ns_sum: VALIDATION_LATENCY_NS.load(Ordering::Relaxed),
        compile_samples: COMPILE_SAMPLES.load(Ordering::Relaxed),
        compile_latency_ns_sum: COMPILE_LATENCY_NS.load(Ordering::Relaxed),
    }
}
