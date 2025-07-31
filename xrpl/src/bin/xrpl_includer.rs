use dotenv::dotenv;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::config::config_from_yaml;
use relayer_base::{
    database::PostgresDB,
    gmp_api,
    payload_cache::PayloadCache,
    queue::Queue,
    utils::setup_heartbeat,
};
use relayer_base::logging::setup_logging;
use xrpl::{
    client::XRPLClient, config::XRPLConfig, includer::XrplIncluder,
    models::queued_transactions::PgQueuedTransactionsModel,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let construct_proof_queue =
        Queue::new(&config.common_config.queue_address, "construct_proof").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true)?);
    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    let payload_cache = PayloadCache::new(postgres_db.clone());
    let xrpl_client = XRPLClient::new(&config.xrpl_rpc, 3)?;
    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let queued_tx_model = PgQueuedTransactionsModel::new(pg_pool.clone());
    let xrpl_includer = XrplIncluder::new::<XRPLClient, PostgresDB, PgQueuedTransactionsModel>(
        config.clone(),
        gmp_api,
        redis_pool.clone(),
        payload_cache,
        Arc::clone(&construct_proof_queue),
        queued_tx_model.clone(),
        Arc::new(xrpl_client),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create XrplIncluder: {}", e))?;

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:includer".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = xrpl_includer.run(Arc::clone(&tasks_queue)) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
