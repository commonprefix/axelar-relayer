use dotenv::dotenv;

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use solana::{
    client::SolanaClient, config::SolanaConfig, models::solana_subscriber_cursor::PostgresDB,
    subscriber::SolanaSubscriber,
};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let postgres_cursor = PostgresDB::new(&config.common_config.postgres_url).await?;

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let solana_client: SolanaClient =
        SolanaClient::new(&config.solana_rpc, config.solana_commitment, 3)?;
    let solana_subscriber =
        SolanaSubscriber::new(solana_client, "default".to_string(), postgres_cursor).await?;

    let mut subscriber = Subscriber::new(solana_subscriber);
    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:subscriber".to_owned(), redis_conn);

    let account = Pubkey::from_str(&config.solana_multisig)?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.run(account, Arc::clone(&events_queue)) => {},
    }

    events_queue.close().await;

    Ok(())
}
