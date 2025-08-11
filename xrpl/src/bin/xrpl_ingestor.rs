use dotenv::dotenv;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use xrpl::{
    ingestor::{XrplIngestor, XrplIngestorModels},
    xrpl_transaction::PgXrplTransactionModel,
};

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    database::PostgresDB,
    gmp_api,
    ingestor::Ingestor,
    models::task_retries::PgTaskRetriesModel,
    payload_cache::PayloadCache,
    price_view::PriceView,
    queue::Queue,
    utils::setup_heartbeat,
};
use relayer_base::logging::setup_logging;
use xrpl::config::XRPLConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let (_sentry_guard, otel_guard) = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;

    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let gmp_api = gmp_api::construct_gmp_api(pg_pool.clone(), &config.common_config, true)?;

    let price_view = PriceView::new(postgres_db.clone());
    let payload_cache = PayloadCache::new(postgres_db.clone());
    let models = XrplIngestorModels {
        xrpl_transaction_model: PgXrplTransactionModel::new(pg_pool.clone()),
        task_retries: PgTaskRetriesModel::new(pg_pool.clone()),
    };
    // make an xrpl ingestor
    let xrpl_ingestor = XrplIngestor::new(
        Arc::clone(&gmp_api),
        config.clone(),
        price_view,
        payload_cache,
        models,
    );
    let ingestor = Ingestor::new(gmp_api, xrpl_ingestor);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:ingestor".to_owned(), redis_conn);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ingestor.run(Arc::clone(&events_queue), Arc::clone(&tasks_queue)) => {},
    }

    tasks_queue.close().await;
    events_queue.close().await;

    otel_guard
        .force_flush()
        .expect("Failed to flush OTEL messages");

    Ok(())
}
