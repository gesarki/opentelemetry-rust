use http_body_util::Full;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use opentelemetry::{
    global,
    trace::{SpanKind, TraceContextExt, Tracer},
    Context,
};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_http::{Bytes, HeaderInjector};
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, propagation::TraceContextPropagator, trace::SdkTracerProvider, Resource
};
use opentelemetry_stdout::{LogExporter};
use opentelemetry_otlp::{MetricExporter, SpanExporter};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::sync::OnceLock;

fn get_resource() -> Resource {
    static RESOURCE: OnceLock<Resource> = OnceLock::new();
    RESOURCE
        .get_or_init(|| {
            Resource::builder()
                .with_service_name("basic-otlp-example-grpc")
                .build()
        })
        .clone()
}

fn init_tracer() -> SdkTracerProvider {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create span exporter");
    global::set_text_map_propagator(TraceContextPropagator::new());
    SdkTracerProvider::builder()
        .with_resource(get_resource())
        .with_batch_exporter(exporter)
        .build()
    // // Install stdout exporter pipeline to be able to retrieve the collected spans.
    // // For the demonstration, use `Sampler::AlwaysOn` sampler to sample all traces.
    // let provider = SdkTracerProvider::builder()
    //     .with_simple_exporter(SpanExporter::default())
    //     .build();

    // global::set_tracer_provider(provider.clone());
    // provider
}

fn init_logs() -> SdkLoggerProvider {
    // Setup tracerprovider with stdout exporter
    // that prints the spans to stdout.
    let logger_provider = SdkLoggerProvider::builder()
        .with_simple_exporter(LogExporter::default())
        .build();
    let otel_layer = OpenTelemetryTracingBridge::new(&logger_provider);
    tracing_subscriber::registry()
        .with(otel_layer)
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .init();

    logger_provider
}

async fn send_request(
    url: &str,
    body_content: &str,
    span_name: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let client = Client::builder(TokioExecutor::new()).build_http();
    let tracer = global::tracer("example/client");
    let span = tracer
        .span_builder(String::from(span_name))
        .with_kind(SpanKind::Client)
        .start(&tracer);
    let cx = Context::current_with_span(span);

    let mut req = hyper::Request::builder().uri(url);
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut HeaderInjector(req.headers_mut().unwrap()))
    });
    req.headers_mut()
        .unwrap()
        .insert("baggage", "is_synthetic=true".parse().unwrap());
    let res = client
        .request(req.body(Full::new(Bytes::from(body_content.to_string())))?)
        .await?;

    info!(name: "ResponseReceived", status = res.status().to_string(), message = "Response received");

    Ok(())
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let tracer_provider = init_tracer();
    global::set_tracer_provider(tracer_provider.clone());
    let logger_provider = init_logs();

    send_request(
        "http://127.0.0.1:3000/health",
        "Health Request!",
        "server_health_check",
    )
    .await?;
    send_request(
        "http://127.0.0.1:3000/echo",
        "Echo Request!",
        "server_echo_check",
    )
    .await?;

    tracer_provider
        .shutdown()
        .expect("Shutdown provider failed");
    logger_provider
        .shutdown()
        .expect("Shutdown provider failed");
    Ok(())
}
