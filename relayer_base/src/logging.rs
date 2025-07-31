use sentry::ClientInitGuard;
use tracing_subscriber::{fmt, Layer, Registry};
use tracing::level_filters::LevelFilter;
use sentry_tracing::{layer as sentry_layer, EventFilter};
use tracing::Level;
use opentelemetry::global;
use opentelemetry::global::BoxedSpan;
use opentelemetry::trace::Span;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use crate::config::Config;
use sentry::integrations::opentelemetry as sentry_opentelemetry;
use serde_json::{json, Value};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use opentelemetry::{SpanId, TraceFlags, TraceId, trace::SpanContext, trace::TraceState};
use anyhow;

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


pub fn serialize_span(span: &mut BoxedSpan) -> String {
    let span_context = span.span_context();

    let json_context = json!({
                        "trace_id": span_context.trace_id().to_string(),
                        "span_id": span_context.span_id().to_string(),
                        "trace_flags": format!("{:?}", span_context.trace_flags()),
                        "is_remote": span_context.is_remote(),
                        "trace_state": span_context.trace_state().header()
                    });
    let serialized_context = json_context.to_string();
    serialized_context
}

pub fn deserialize_span_context(json_str: &str) -> Result<SpanContext, anyhow::Error> {
    let parsed: Value = serde_json::from_str(json_str)?;

    let trace_id_str = parsed["trace_id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing trace_id"))?;
    let span_id_str = parsed["span_id"].as_str().ok_or_else(|| anyhow::anyhow!("Missing span_id"))?;
    let trace_flags_str = parsed["trace_flags"].as_str().ok_or_else(|| anyhow::anyhow!("Missing trace_flags"))?;
    let is_remote = parsed["is_remote"].as_bool().ok_or_else(|| anyhow::anyhow!("Missing is_remote"))?;
    let trace_state_str = parsed["trace_state"].as_str().ok_or_else(|| anyhow::anyhow!("Missing trace_state"))?;

    let trace_id = TraceId::from_hex(trace_id_str)?;
    let span_id = SpanId::from_hex(span_id_str)?;

    let trace_flags_value = if trace_flags_str.contains("0x01") {
        TraceFlags::SAMPLED
    } else {
        TraceFlags::NOT_SAMPLED
    };

    let trace_state = TraceState::from_str(trace_state_str)?;

    Ok(SpanContext::new(
        trace_id,
        span_id,
        trace_flags_value,
        is_remote,
        trace_state,
    ))
}
