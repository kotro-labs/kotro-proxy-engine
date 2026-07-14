//! OpenTelemetry tracing pipeline initialisation.
//!
//! Call `init_telemetry` once at startup. If `KOTRO_OTEL_ENDPOINT` is set,
//! spans are exported via OTLP/HTTP to that endpoint. Otherwise the process
//! falls back to stdout JSON logging.
//!
//! The returned `Option<SdkTracerProvider>` must be stored by the caller and
//! its `shutdown()` method called on SIGTERM/SIGINT so buffered spans flush
//! before the process exits.

use anyhow::Result;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace, Resource};
use opentelemetry_semantic_conventions::resource;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Initializes the tracing pipeline.
///
/// Returns `Some(provider)` when OTel is active so the caller can shut it down
/// cleanly; returns `None` when using stdout-only logging.
pub fn init_telemetry(otel_endpoint: Option<&str>) -> Result<Option<trace::SdkTracerProvider>> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(endpoint) = otel_endpoint {
        let resource = Resource::builder()
            .with_service_name("kotro-proxy")
            .with_attributes(vec![
                KeyValue::new(resource::SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
            ])
            .build();

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build OTLP exporter: {}", e))?;

        let provider = trace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();

        global::set_tracer_provider(provider.clone());
        let tracer = global::tracer("kotro-proxy");
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

        Registry::default()
            .with(env_filter)
            .with(telemetry)
            .with(tracing_subscriber::fmt::layer().json())
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to init tracing subscriber: {}", e))?;

        Ok(Some(provider))
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to init tracing subscriber: {}", e))?;

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-test: init_telemetry with no endpoint must not panic.
    /// (We can only call try_init once per process; subsequent calls return an
    /// error which we intentionally ignore here.)
    #[test]
    fn init_without_endpoint_does_not_panic() {
        // May return error if a subscriber is already set (e.g., in parallel tests).
        let _ = init_telemetry(None);
    }
}
