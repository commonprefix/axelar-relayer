use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::{
    config::Config,
    database::PostgresDB,
    utils::{setup_heartbeat, setup_logging},
};
use xrpl::{client::XRPLClient, queued_tx_monitor::XrplQueuedTxMonitor};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config);

    let postgres_db = PostgresDB::new(&config.postgres_url).await?;
    let xrpl_client = Arc::new(XRPLClient::new(&config.xrpl_rpc, 3)?);

    let xrpl_tx_monitor = XrplQueuedTxMonitor::new(xrpl_client, postgres_db);

    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;
    setup_heartbeat("heartbeat:queued_tx_monitor".to_owned(), redis_pool);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
        _ = xrpl_tx_monitor.run() => {},
    }

    Ok(())
}
