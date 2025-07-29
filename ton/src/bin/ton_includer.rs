use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::utils::setup_heartbeat;
use relayer_base::{
    database::PostgresDB, gmp_api, payload_cache::PayloadCache, queue::Queue, utils::setup_logging,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use ton::config::TONConfig;
use ton::high_load_query_id_db_wrapper::HighLoadQueryIdDbWrapper;
use ton::includer::TONIncluder;
use ton::ton_wallet_query_id::PgTONWalletQueryIdModel;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{network}.yaml"))?;

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let construct_proof_queue =
        Queue::new(&config.common_config.queue_address, "construct_proof").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true)?);
    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    let payload_cache_for_includer = PayloadCache::new(postgres_db);

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let model = PgTONWalletQueryIdModel::new(pg_pool);

    let high_load_query_id_wrapper = HighLoadQueryIdDbWrapper::new(model).await;
    let ton_includer = TONIncluder::new(
        config,
        gmp_api,
        redis_pool.clone(),
        payload_cache_for_includer,
        construct_proof_queue.clone(),
        Arc::new(high_load_query_id_wrapper),
    )
    .await
    .unwrap();
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:includer".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ton_includer.run(tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
