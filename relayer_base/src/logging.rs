use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::env;
use lapin::message::Delivery;
use lapin::types::{AMQPValue, ShortString};
use crate::config::Config;
use opentelemetry::{global, Context};
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use sentry::ClientInitGuard;
use sentry_tracing::{EventFilter};
use tracing::level_filters::LevelFilter;
use tracing::{info, Level, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, Layer};
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
    global::set_text_map_propagator(TraceContextPropagator::new());


    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint("http://localhost:4317")
        .build().unwrap();

    let path = match env::current_exe() {
        Ok(exe_path) => exe_path.file_name().expect("missing file name").to_str().expect("missing file name").to_string(),
        Err(_) => "<unknown>".to_string(),
    };

    let resource = Resource::builder()
        .with_service_name(path)
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

    let sentry_layer = sentry::integrations::tracing::layer()
        .event_filter(|metadata| match *metadata.level() { 
            Level::ERROR => EventFilter::Event, 
            Level::WARN => EventFilter::Event, 
            _ => EventFilter::Breadcrumb})
        .span_filter(|md| matches!(*md.level(), Level::INFO | Level::WARN | Level::ERROR));

    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(LevelFilter::INFO);

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

pub fn distributed_tracing_headers(span: &Span) -> BTreeMap<ShortString, AMQPValue> {
    let mut headers = BTreeMap::new();

    global::get_text_map_propagator(|propagator| {
        let context = span.context();
        propagator.inject_context(&context, &mut HeadersBTreeMap(&mut headers));
    });

    headers
}

pub fn distributed_tracing_extract_parent_context(delivery: &Delivery) -> Context {
    let mut headers_map = HashMap::new();
    if let Some(headers) = delivery.properties.headers() {
        for (key, value) in headers.inner().iter() {
            if let Some(value_str) = value.as_long_string() {
                headers_map.insert(key.to_string(), value_str.to_string());
            }
        }
    }
    let parent_cx =
        global::get_text_map_propagator(|prop| prop.extract(&HeadersMap(&mut headers_map)));

    parent_cx
}


pub struct HeadersBTreeMap<'a>(pub &'a mut BTreeMap<ShortString, AMQPValue>);
pub struct HeadersMap<'a>(&'a mut HashMap<String, String>);

impl Injector for HeadersBTreeMap<'_> {
    fn set(&mut self, key: &str, value: String) {
        let key = ShortString::from(key);
        let val = AMQPValue::LongString(value.into());
        self.0.insert(key, val);
    }
}

impl Extractor for HeadersMap<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|metadata| Option::from(metadata.as_str()))
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
    }
}
