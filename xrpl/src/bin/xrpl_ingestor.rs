use dotenv::dotenv;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use xrpl::{
    ingestor::{XrplIngestor, XrplIngestorModels},
    xrpl_transaction::PgXrplTransactionModel,
};

use relayer_base::config::config_from_yaml;
use relayer_base::{
    database::PostgresDB,
    gmp_api,
    ingestor::Ingestor,
    models::task_retries::PgTaskRetriesModel,
    payload_cache::PayloadCache,
    price_view::PriceView,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};
use xrpl::config::XRPLConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url)
        .await
        .unwrap();
    let pg_pool = PgPool::connect(&config.common_config.postgres_url)
        .await
        .unwrap();
    let price_view = PriceView::new(postgres_db.clone());
    let payload_cache = PayloadCache::new(postgres_db.clone());
    let models = XrplIngestorModels {
        xrpl_transaction_model: PgXrplTransactionModel::new(pg_pool.clone()),
        task_retries: PgTaskRetriesModel::new(pg_pool.clone()),
    };
    // make an xrpl ingestor
    let xrpl_ingestor = XrplIngestor::new(
        gmp_api.clone(),
        config.clone(),
        price_view,
        payload_cache,
        models,
    );
    let ingestor = Ingestor::new(gmp_api, xrpl_ingestor);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:ingestor".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ingestor.run(events_queue.clone(), tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    events_queue.close().await;

    Ok(())
}
