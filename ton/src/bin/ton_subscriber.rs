use sentry::{Hub, TransactionOrSpan};
use std::collections::BTreeMap;
use std::io;
use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::error::SubscriberError;
use relayer_base::queue::Queue;
use relayer_base::redis::connection_manager;
use relayer_base::subscriber::Subscriber;
use relayer_base::utils::setup_heartbeat;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use opentelemetry::{global, Context, KeyValue};
use opentelemetry::context::FutureExt;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TraceContextExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;
use ton::client::TONRpcClient;
use ton::config::TONConfig;
use ton::retry_subscriber::RetryTONSubscriber;
use ton::subscriber::TONSubscriber;
use ton::ton_trace::PgTONTraceModel;
use tonlib_core::TonAddress;
use tracing::{error, info, info_span, span, warn, warn_span, Instrument, Span};
use relayer_base::logging::setup_logging;
use tracing_opentelemetry::OpenTelemetrySpanExt;


#[tracing::instrument]
async fn inner(i: u32) {
    global::get_text_map_propagator(|propagator| {
        let context = Span::current().context();
        println!("{context:?}");
    });

    // Also works, since log events are ingested by the tracing system
    tracing::warn!(number = i, "Generates a breadcrumb");
    let s = info_span!("Programmatic inner");

    // // Create a Sentry span for the inner function
    // let inner_span = sentry::configure_scope(|scope| {
    //     if let Some(span) = scope.get_span() {
    //         Some(span.start_child("inner_function", "inner"))
    //     } else {
    //         None
    //     }
    // });

    async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        for i in 0..2 {
            info!("Some piece of info");
            let loop_span = info_span!("inner loop", l_iter = i);
            error!(error = "category", "connect failed");
            let context = Span::current().context();
            println!("{context:?}");
        }
    }.instrument(s).await;

    // // Finish the inner Sentry span if it exists
    // if let Some(span) = inner_span {
    //     span.finish();
    // }
}


async fn outer(ctx: opentelemetry::Context, sentry_headers: Vec<(&str, &str)>, mut headers: BTreeMap<ShortString, AMQPValue>) {
    //println!("got context: {ctx:?}");
    println!("got sentry_headers: {sentry_headers:?}");
    let parent_cx =
        global::get_text_map_propagator(|prop| prop.extract(&HeadersMap(&mut headers)));


    println!("got headers: {:?}", headers);
    println!("parent_cx = {:?}", parent_cx);


    let tx_ctx = sentry::TransactionContext::continue_from_headers(
        "ingestor",
        "consume_task",
        sentry_headers.clone(),
    );
    let transaction = sentry::start_transaction(tx_ctx);
    let tc = transaction.get_trace_context();
    let span = transaction.start_child("entry", "my entry");
    // Clone the span and transaction so we can finish them later
    let span_clone = span.clone();
    let transaction_clone = transaction.clone();

    Hub::current().configure_scope(|scope| {
        let curr_span = scope.get_span();
        println!("new scope: {curr_span:?}");
        scope.set_span(Some(TransactionOrSpan::Span(span.clone())));
    });



    // let transaction = sentry::start_transaction(tx_ctx);
    // let tc = transaction.get_trace_context();
    // let span = transaction.start_child("entry", "my entry");
    // span.finish();



    let s: Span = info_span!("Programmatic outer");
    s.set_parent(parent_cx);
    // for (k, v) in sentry_headers {
    //     println!("{k}: {v}");
    //     s.set_attribute((*k).to_string(), (*v).to_string());
    // }


    async move {
        // Now, inside the instrumented future, the current span is `s`,
        // and its OpenTelemetry context is a child of `ctx`.

        for i in 0..2 {
            inner(i).await;
        }
    }
        .instrument(s)
        .await;

    // Finish the Sentry span to ensure it's recorded and sent to Sentry
    span_clone.finish();
    transaction_clone.finish();
}

use lapin::types::{AMQPValue, FieldTable, ShortString};
use tracing::log::kv::{Key, Source};

struct HeadersMap<'a>(&'a mut BTreeMap<ShortString, AMQPValue>);
impl Injector for HeadersMap<'_> {
    fn set(&mut self, key: &str, value: String) {
        let key = ShortString::from(key);
        let val = AMQPValue::LongString(value.into());
        self.0.insert(key, val);
    }
}

impl Extractor for HeadersMap<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        let res = self.0.get(key).and_then(|metadata| {
            match metadata {
                AMQPValue::LongString(s) => Some(std::str::from_utf8(s.as_bytes()).ok()?),
                _ => None,
            }
        });

        res
    }

    fn keys(&self) -> Vec<&str> {
        let keys = self.0
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>();

        keys
    }
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{network}.yaml"))?;

    let (_sentry_guard, otel_guard) = setup_logging(&config.common_config);

    let root = tracing::info_span!("parent");
    let mut headers = BTreeMap::new();

    let (ctx, sentry_headers_string) = {
        let _e = root.enter();
        let mut sentry_headers = vec![];

        if let Some(span) = sentry::configure_scope(|scope| scope.get_span()) {
            for (k, v) in span.iter_headers() {
                sentry_headers.push((k, v));
            }
        }
        let ctx = Span::current().context();

        global::get_text_map_propagator(|propagator| {
            let context = Span::current().context();
            println!("inner ctx: {context:?}");
            propagator.inject_context(&context, &mut HeadersMap(&mut headers));
        });
        Hub::current().configure_scope(|scope| {
            println!("old scope: {scope:?}");
        });

        println!("my ctx: {ctx:?}");
        println!("my headers: {headers:?}");
        (ctx, sentry_headers)
    };
    drop(root);

    // Convert Vec<(&str, String)> to Vec<(&str, &str)>
    let sentry_headers: Vec<(&str, &str)> = sentry_headers_string
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();


    println!("{:?}", headers);

    outer(ctx, sentry_headers, headers).await;
    warn!(name: "testing_warn", "foo");
    otel_guard.force_flush()?;
    return Ok(());
    // let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    // let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    //
    // let ton_gateway = config.ton_gateway;
    // let ton_gas_service = config.ton_gas_service;
    // let gateway_account = TonAddress::from_base64_url(ton_gateway.as_str())?;
    // let gas_service_account = TonAddress::from_base64_url(ton_gas_service.as_str())?;
    //
    // let mut sigint = signal(SignalKind::interrupt())?;
    // let mut sigterm = signal(SignalKind::terminate())?;
    //
    // let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    // let redis_conn = connection_manager(redis_client, None, None, None).await?;
    //
    // setup_heartbeat("heartbeat:subscriber".to_owned(), redis_conn);
    //
    // let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    //
    // let ton_traces = PgTONTraceModel::new(pg_pool.clone());
    //
    // let mut handles: Vec<JoinHandle<()>> = vec![];
    //
    // let client = TONRpcClient::new(config.ton_rpc.clone(), config.ton_api_key.clone(), 5, 5, 30)
    //     .await
    //     .map_err(|e| error_stack::report!(SubscriberError::GenericError(e.to_string())))
    //     .expect("Failed to create RPC client");
    //
    // for acct in [gateway_account.clone(), gas_service_account] {
    //     let ton_sub = TONSubscriber::new(
    //         client.clone(),
    //         postgres_db.clone(),
    //         acct.to_string(),
    //         config.common_config.chain_name.clone(),
    //         ton_traces.clone(),
    //     )
    //     .await?;
    //
    //     let mut sub = Subscriber::new(ton_sub);
    //     let queue_clone = Arc::clone(&events_queue);
    //     let handle = tokio::spawn(async move {
    //         sub.run(acct, queue_clone).await;
    //     });
    //     handles.push(handle);
    // }
    //
    // let retry_subscriber = RetryTONSubscriber::new(client.clone(), ton_traces.clone()).await?;
    // let mut sub = Subscriber::new(retry_subscriber);
    // let events_clone = Arc::clone(&events_queue);
    // let handle = tokio::spawn(async move {
    //     sub.run(gateway_account, events_clone).await;
    // });
    // handles.push(handle);
    //
    // tokio::select! {
    //     _ = sigint.recv()  => {},
    //     _ = sigterm.recv() => {},
    // }
    //
    // for handle in handles {
    //     handle.abort();
    // }
    //
    // events_queue.close().await;
    //
    // Ok(())
}
