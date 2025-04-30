use dotenv::dotenv;
use tokio::signal::unix::{signal, SignalKind};

use axelar_relayer::{
    config::Config,
    payload_cache::PayloadCache,
    proof_retrier::ProofRetrier,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.proof_retrier.clone());

    let construct_proof_queue = Queue::new(&config.queue_address, "construct_proof").await;
    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let payload_cache = PayloadCache::new(redis_pool.clone());
    let proof_retrier = ProofRetrier::new(
        payload_cache,
        construct_proof_queue.clone(),
        tasks_queue.clone(),
    );

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = proof_retrier.run() => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
