use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::any::Any;
use crate::config::Config;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use sentry::integrations::opentelemetry as sentry_opentelemetry;
use sentry::ClientInitGuard;
use sentry_tracing::{layer as sentry_layer, EventFilter};
use tracing::level_filters::LevelFilter;
use tracing::{info, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, Layer, Registry};
use tracing_subscriber::util::SubscriberInitExt;
use xrpl_api::Transaction;
use xrpl_types::AccountId;
use ton_types::ton_types::Trace;

pub fn setup_logging(config: &Config) -> (ClientInitGuard, SdkTracerProvider) {
    let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

    let guard = sentry::init((
        config.sentry_dsn.to_string(),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            environment: Some(std::borrow::Cow::Owned(environment)),
            traces_sample_rate: 1.0,
            ..Default::default()
        },
    ));

    //global::set_text_map_propagator(sentry_opentelemetry::SentryPropagator::new());
    
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint("http://localhost:4317")
        .build().unwrap();

    let resource = Resource::builder()
        .with_service_name("relayer_service")
        .build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            1.0,
        ))))
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = tracer_provider.tracer("relayer");
    
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_filter(LevelFilter::DEBUG);

    let sentry_layer = sentry_layer().event_filter(|metadata| match *metadata.level() {
        Level::ERROR => EventFilter::Event,
        Level::WARN => EventFilter::Event,
        _ => EventFilter::Breadcrumb,
    });

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(fmt_layer) // Console logging
        .with(sentry_layer) // Sentry logging
        .with(otel_layer) // Otel, required for Sentry tracing too
        .init();
    
    (guard, tracer_provider)
}

pub fn maybe_to_string(val: &dyn Any) -> Option<String> {
    if let Some(t) = val.downcast_ref::<Trace>() {
        return Some(t.trace_id.to_string());
    } else if let Some(t) = val.downcast_ref::<Transaction>() {
        return t.common().hash.clone();
    } else if let Some(t) = val.downcast_ref::<AccountId>() {
        return Some(t.to_address());
    } else {
        info!("Unknown type");
    }

    None
}
