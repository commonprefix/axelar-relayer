use dotenv::dotenv;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::config::{config_from_yaml, Config};
use relayer_base::{
    database::PostgresDB,
    payload_cache::PayloadCache,
    proof_retrier::ProofRetrier,
    queue::Queue,
    utils::setup_heartbeat,
};
use std::sync::Arc;
use relayer_base::logging::setup_logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: Config = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config);

    let construct_proof_queue = Queue::new(&config.queue_address, "construct_proof").await;
    let tasks_queue = Queue::new(&config.queue_address, "ingestor_tasks").await;
    let postgres_db = PostgresDB::new(&config.postgres_url).await?;
    let payload_cache = PayloadCache::new(postgres_db);
    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;
    let proof_retrier = ProofRetrier::new(
        payload_cache,
        Arc::clone(&construct_proof_queue),
        Arc::clone(&tasks_queue),
        redis_pool.clone(),
    );

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:proof_retrier".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = proof_retrier.run() => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
