use std::sync::Arc;
use dotenv::dotenv;
use tokio::signal::unix::{signal, SignalKind};
use sqlx::PgPool;

use relayer_base::config::config_from_yaml;
use relayer_base::{
    database::PostgresDB,
    distributor::Distributor,
    gmp_api,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::redis::connection_manager;
use xrpl::config::XRPLConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let includer_tasks_queue =
        Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let ingestor_tasks_queue =
        Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create PostgresDB: {}", e))?;

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let gmp_api = gmp_api::construct_gmp_api(pg_pool, &config.common_config, true)?;

    let mut distributor = Distributor::new(
        postgres_db,
        "default".to_string(),
        gmp_api,
        config.common_config.refunds_enabled,
    )
    .await;

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;
    
    setup_heartbeat("heartbeat:distributor".to_owned(), redis_conn);

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

    Ok(())
}
