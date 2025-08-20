use dotenv::dotenv;
use solana::client::SolanaClient;
use solana::config::SolanaConfig;
use solana::includer::SolanaIncluder;
use solana_sdk::commitment_config::CommitmentConfig;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    database::PostgresDB,
    gmp_api,
    models::gmp_events::PgGMPEvents,
    models::gmp_tasks::PgGMPTasks,
    payload_cache::PayloadCache,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let construct_proof_queue =
        Queue::new(&config.common_config.queue_address, "construct_proof").await;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let payload_cache = PayloadCache::new(postgres_db.clone());
    let solana_client = SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3)?;
    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let gmp_api = gmp_api::construct_gmp_api(pg_pool.clone(), &config.common_config, true)?;

    let solana_includer = SolanaIncluder::new::<
        SolanaClient,
        PostgresDB,
        gmp_api::GmpApiDbAuditDecorator<gmp_api::GmpApi, PgGMPTasks, PgGMPEvents>,
    >(
        config.clone(),
        gmp_api,
        redis_conn.clone(),
        payload_cache,
        Arc::clone(&construct_proof_queue),
        Arc::new(solana_client),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create SolanaIncluder: {}", e))?;

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    setup_heartbeat("heartbeat:includer".to_owned(), redis_conn);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = solana_includer.run(Arc::clone(&tasks_queue)) => {},
    }

    tasks_queue.close().await;
    construct_proof_queue.close().await;

    Ok(())
}
