use dotenv::dotenv;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use xrpl::{ingestor::XrplIngestor, xrpl_transaction::PgXrplTransactionModel};

use relayer_base::{
    config::Config,
    database::PostgresDB,
    gmp_api,
    ingestor::{Ingestor, IngestorModels},
    models::task_retries::PgTaskRetriesModel,
    payload_cache::PayloadCache,
    price_view::PriceView,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let events_queue = Queue::new(&config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let pg_pool = PgPool::connect(&config.postgres_url).await.unwrap();
    let price_view = PriceView::new(postgres_db.clone());
    let payload_cache = PayloadCache::new(postgres_db.clone());
    let xrpl_transaction_model = PgXrplTransactionModel {
        pool: pg_pool.clone(),
    };
    // make an xrpl ingestor
    let xrpl_ingestor = XrplIngestor::new(
        gmp_api.clone(),
        config.clone(),
        price_view,
        payload_cache,
        xrpl_transaction_model,
    );
    let models = IngestorModels {
        task_retries: PgTaskRetriesModel {
            pool: pg_pool.clone(),
        },
    };
    let ingestor = Ingestor::new(gmp_api, xrpl_ingestor, models);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.redis_server.clone())?;
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
