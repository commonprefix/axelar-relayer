use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::gmp_api::gmp_types::TaskKind;
use relayer_base::logging::setup_logging;
use relayer_base::logging_ctx_cache::RedisLoggingCtxCache;
use relayer_base::redis::connection_manager;
use relayer_base::{
    database::PostgresDB, distributor::Distributor, gmp_api, queue::Queue, utils::setup_heartbeat,
};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use ton::config::TONConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{network}.yaml"))?;

    let (_sentry_guard, otel_guard) = setup_logging(&config.common_config);

    let includer_tasks_queue = Queue::new(
        &config.common_config.queue_address,
        "includer_tasks",
        config.common_config.num_workers,
    )
    .await;
    let ingestor_tasks_queue = Queue::new(
        &config.common_config.queue_address,
        "ingestor_tasks",
        config.common_config.num_workers,
    )
    .await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let gmp_api = gmp_api::construct_gmp_api(pg_pool, &config.common_config, true)?;
    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;
    let logging_ctx_cache = RedisLoggingCtxCache::new(redis_conn.clone());

    let mut distributor = Distributor::new(
        postgres_db,
        "default".to_string(),
        gmp_api,
        config.common_config.refunds_enabled,
        Arc::new(logging_ctx_cache),
    )
    .await;
    distributor.set_supported_includer_tasks(vec![
        TaskKind::Refund,
        TaskKind::Execute,
        TaskKind::GatewayTx,
    ]);

    setup_heartbeat("heartbeat:distributor".to_owned(), redis_conn, None);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = distributor.run(
            Arc::clone(&includer_tasks_queue),
            Arc::clone(&ingestor_tasks_queue),
        ) => {},
    }

    ingestor_tasks_queue.close().await;
    includer_tasks_queue.close().await;

    otel_guard
        .force_flush()
        .expect("Failed to flush OTEL messages");

    Ok(())
}
