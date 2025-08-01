use dotenv::dotenv;

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    database::PostgresDB,
    queue::Queue,
    subscriber::Subscriber,
    utils::setup_heartbeat,
};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use xrpl::{client::XRPLClient, config::XRPLConfig, subscriber::XrplSubscriber};
use xrpl_types::AccountId;
use relayer_base::logging::setup_logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let account = AccountId::from_address(&config.xrpl_multisig)?;

    let xrpl_client = XRPLClient::new(&config.xrpl_rpc, 3)?;
    let xrpl_subscriber =
        XrplSubscriber::new(xrpl_client, postgres_db, "default".to_string()).await?;
    let mut subscriber = Subscriber::new(xrpl_subscriber);
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:subscriber".to_owned(), redis_conn);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.run(account, Arc::clone(&events_queue)) => {},
    }

    events_queue.close().await;

    Ok(())
}
