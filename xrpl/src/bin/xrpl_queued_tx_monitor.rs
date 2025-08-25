use dotenv::dotenv;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::redis::connection_manager;
use relayer_base::{
    config::config_from_yaml,
    utils::{setup_heartbeat, setup_logging},
};
use xrpl::{
    client::XRPLClient, config::XRPLConfig, models::queued_transactions::PgQueuedTransactionsModel,
    queued_tx_monitor::XrplQueuedTxMonitor,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;
    let xrpl_client = Arc::new(XRPLClient::new(&config.xrpl_rpc, 3)?);
    let queued_tx_model = PgQueuedTransactionsModel::new(pg_pool);

    let xrpl_tx_monitor = XrplQueuedTxMonitor::new(xrpl_client, queued_tx_model);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;
    setup_heartbeat("heartbeat:queued_tx_monitor".to_owned(), redis_conn, None);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
        _ = xrpl_tx_monitor.run() => {},
    }

    Ok(())
}
