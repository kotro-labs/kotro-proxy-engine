use anyhow::Result;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace, Resource};
use opentelemetry_semantic_conventions::resource;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Initializes the OpenTelemetry tracing pipeline if an endpoint is provided.
/// Otherwise, falls back to standard stdout JSON tracing.
pub fn init_telemetry(otel_endpoint: Option<&str>) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(endpoint) = otel_endpoint {
        // Resource creation in OTel 0.22/0.23/0.32
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
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .json()
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to init tracing subscriber: {}", e))?;
    }

    Ok(())
}
