use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::logging::setup_logging;
use relayer_base::redis::connection_manager;
use relayer_base::utils::setup_heartbeat;
use relayer_base::{database::PostgresDB, gmp_api, payload_cache::PayloadCache, queue::Queue};
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

    let (_sentry_guard, otel_guard) = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let construct_proof_queue =
        Queue::new(&config.common_config.queue_address, "construct_proof").await;
    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client.clone(), None, None, None).await?;

    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    let payload_cache_for_includer = PayloadCache::new(postgres_db);

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let model = PgTONWalletQueryIdModel::new(pg_pool.clone());
    let gmp_api = gmp_api::construct_gmp_api(pg_pool.clone(), &config.common_config, true)?;

    let high_load_query_id_wrapper = HighLoadQueryIdDbWrapper::new(model).await;
    let ton_includer = TONIncluder::new(
        config,
        gmp_api,
        redis_conn.clone(),
        payload_cache_for_includer,
        Arc::clone(&construct_proof_queue),
        Arc::new(high_load_query_id_wrapper),
    )
    .await
    .expect("Failed to construct TONIncluder");
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:includer".to_owned(), redis_conn);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ton_includer.run(Arc::clone(&tasks_queue)) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    otel_guard
        .force_flush()
        .expect("Failed to flush OTEL messages");

    Ok(())
}
