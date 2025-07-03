use std::sync::Arc;
use sqlx::PgPool;
use dotenv::dotenv;
use tokio::signal::unix::{signal, SignalKind};
use relayer_base::config::Config;
use relayer_base::database::PostgresDB;
use relayer_base::gmp_api;
use relayer_base::ingestor::Ingestor;
use relayer_base::payload_cache::PayloadCache;
use relayer_base::price_view::PriceView;
use relayer_base::queue::Queue;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use ton::ingestor::TONIngestor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let tasks_queue = Queue::new(&config.queue_address, "ingestor_tasks").await;
    let events_queue = Queue::new(&config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let pg_pool = PgPool::connect(&config.postgres_url).await.unwrap();
    let price_view = PriceView::new(postgres_db.clone());
    let payload_cache = PayloadCache::new(postgres_db.clone());
    
    let ton_ingestor: TONIngestor<PostgresDB> = TONIngestor::new();
    let ingestor = Ingestor::new(gmp_api, ton_ingestor);
    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:ingestor".to_owned(), redis_pool);
    
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ingestor.run(events_queue.clone(), tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    events_queue.close().await;



    Ok(())
}