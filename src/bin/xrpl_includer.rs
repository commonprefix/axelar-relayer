use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use axelar_relayer::{
    config::Config,
    database::PostgresDB,
    gmp_api,
    payload_cache::PayloadCache,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
    xrpl::XrplIncluder,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.includer.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let construct_proof_queue = Queue::new(&config.queue_address, "construct_proof").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let payload_cache = PayloadCache::new(postgres_db);
    let xrpl_includer = XrplIncluder::new(
        config,
        gmp_api,
        redis_pool,
        payload_cache,
        construct_proof_queue.clone(),
    )
    .await
    .unwrap();

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = xrpl_includer.run(tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
