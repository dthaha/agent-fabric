//! Observability for the fabric: the fabric EMITS telemetry, it does not
//! store or ship it itself. Structured JSON logs always go to stdout (tailed
//! by whatever collector the customer runs); OTLP traces ship to the
//! customer's existing backend (Tempo, Jaeger, Honeycomb, Datadog, ...) when
//! `OTEL_EXPORTER_OTLP_ENDPOINT` is set. No vendor SDKs, no log backends.

use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use thiserror::Error;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Standard OTLP endpoint env var (OTel spec).
const OTEL_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("failed to build OTLP span exporter: {0}")]
    ExporterBuild(#[from] opentelemetry_otlp::ExporterBuildError),
    #[error("telemetry shutdown failed: {0}")]
    Shutdown(String),
}

pub struct TelemetryConfig {
    pub service_name: String,
    pub service_version: String,
}

/// Holds the TracerProvider so pending spans can be flushed on shutdown.
/// Valid but no-op if the OTLP layer was not enabled.
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl TelemetryGuard {
    /// Flush pending spans and shut down the tracer provider.
    pub fn shutdown(mut self) -> Result<(), TelemetryError> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), TelemetryError> {
        if let Some(provider) = self.provider.take() {
            provider
                .shutdown()
                .map_err(|e| TelemetryError::Shutdown(e.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(e) = self.shutdown_inner() {
            eprintln!("fabric-telemetry: shutdown failed: {e}");
        }
    }
}

/// Initialize the layered tracing subscriber:
/// 1. JSON stdout layer (always on) — the 12-factor / local-dev path.
/// 2. OTLP trace layer (only when OTEL_EXPORTER_OTLP_ENDPOINT is set).
///
/// Verbosity comes from RUST_LOG, defaulting to "info".
pub fn init_telemetry(config: TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let json_layer = tracing_subscriber::fmt::layer().json();

    let (otel_layer, provider) = if std::env::var_os(OTEL_ENDPOINT_ENV).is_some() {
        let endpoint = std::env::var(OTEL_ENDPOINT_ENV).expect("env presence checked");
        let exporter = SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;

        let resource = Resource::builder()
            .with_service_name(config.service_name.clone())
            .with_attribute(KeyValue::new(
                opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
                config.service_version.clone(),
            ))
            .build();

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();

        let tracer =
            opentelemetry::trace::TracerProvider::tracer(&provider, config.service_name.clone());
        let layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
        (Some(layer), Some(provider))
    } else {
        (None, None)
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(json_layer)
        .with(otel_layer)
        .init();

    Ok(TelemetryGuard { provider })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_without_provider_shutdown_ok() {
        let guard = TelemetryGuard { provider: None };
        assert!(guard.shutdown().is_ok());
    }

    #[test]
    fn guard_drop_is_safe() {
        let guard = TelemetryGuard { provider: None };
        drop(guard);
    }
}
