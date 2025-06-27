use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::{
    database::PostgresDB,
    gmp_api,
    payload_cache::PayloadCache,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::config::{config_from_yaml};
use xrpl::config::XRPLConfig;
use xrpl::includer::XrplIncluder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let construct_proof_queue = Queue::new(&config.common_config.queue_address, "construct_proof").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true).unwrap());
    let redis_client = redis::Client::open(config.common_config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await.unwrap();
    let payload_cache = PayloadCache::new(postgres_db);
    let xrpl_includer = XrplIncluder::new(
        config.clone(),
        gmp_api,
        redis_pool.clone(),
        payload_cache,
        construct_proof_queue.clone(),
    )
    .await
    .unwrap();

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:includer".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = xrpl_includer.run(tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
