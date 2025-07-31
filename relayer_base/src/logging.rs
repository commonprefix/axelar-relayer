use crate::config::Config;
use opentelemetry::global;
use opentelemetry_sdk::trace::SdkTracerProvider;
use sentry::integrations::opentelemetry as sentry_opentelemetry;
use sentry::ClientInitGuard;
use sentry_tracing::{layer as sentry_layer, EventFilter};
use tracing::level_filters::LevelFilter;
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, Layer, Registry};

pub fn setup_logging(config: &Config) -> ClientInitGuard {
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

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_filter(LevelFilter::DEBUG);

    let sentry_layer = sentry_layer().event_filter(|metadata| match *metadata.level() {
        Level::ERROR => EventFilter::Event, // Send `error` events to Sentry
        Level::WARN => EventFilter::Event,  // Send `warn` events to Sentry
        _ => EventFilter::Breadcrumb,
    });

    let subscriber = Registry::default()
        .with(fmt_layer) // Console logging
        .with(sentry_layer); // Sentry logging

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global tracing subscriber");

    global::set_text_map_propagator(sentry_opentelemetry::SentryPropagator::new());

    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(sentry_opentelemetry::SentrySpanProcessor::new())
        .build();

    global::set_tracer_provider(tracer_provider);

    guard
}
