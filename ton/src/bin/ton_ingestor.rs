use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::gmp_api;
use relayer_base::ingestor::Ingestor;
use relayer_base::price_view::PriceView;
use relayer_base::queue::Queue;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use ton::config::TONConfig;
use ton::ingestor::TONIngestor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await.unwrap();
    let _pg_pool = PgPool::connect(&config.common_config.postgres_url).await.unwrap();
    let _price_view = PriceView::new(postgres_db.clone());
    
    let ton_ingestor = TONIngestor::new();
    let ingestor = Ingestor::new(gmp_api, ton_ingestor);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
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