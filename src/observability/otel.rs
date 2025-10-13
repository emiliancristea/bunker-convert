use anyhow::Result;
use opentelemetry::sdk::trace as sdktrace;
use opentelemetry::sdk::Resource;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

pub fn init_otel(service_name: &str, endpoint: &str) -> Result<tracing_subscriber::reload::Handle<tracing_subscriber::registry::Registry, tracing_subscriber::Layered<tracing_opentelemetry::OpenTelemetryLayer<Registry>, Registry>>> {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            sdktrace::Config::default().with_resource(Resource::new(vec![KeyValue::new(
                "service.name",
                service_name.to_string(),
            )])),
        )
        .install_batch(opentelemetry::runtime::Tokio)?;

    let layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(layer);
    let (reload_layer, handle) = tracing_subscriber::reload::Layer::new(subscriber);
    tracing_subscriber::registry().with(reload_layer).try_init()?;
    Ok(handle)
}

pub fn shutdown() {
    let _ = opentelemetry::global::shutdown_tracer_provider();
}
