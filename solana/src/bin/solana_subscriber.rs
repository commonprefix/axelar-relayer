use dotenv::dotenv;

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use solana::{
    client::{SolanaRpcClient, SolanaStreamClient},
    config::SolanaConfig,
    models::{solana_signature::PgSolanaSignatureModel, solana_subscriber_cursor::PostgresDB},
    subscriber::{SolanaListener, SolanaPoller},
};
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
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
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;
    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;

    let solana_signature_model = PgSolanaSignatureModel::new(pg_pool.clone());

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let solana_stream_client =
        SolanaStreamClient::new(&config.solana_rpc, config.solana_commitment).await?;

    let solana_rpc_client: SolanaRpcClient =
        SolanaRpcClient::new(&config.solana_rpc, config.solana_commitment, 3)?;

    let solana_poller = SolanaPoller::new(
        solana_rpc_client,
        "solana_poller".to_string(),
        Arc::new(solana_signature_model.clone()),
        Arc::new(postgres_db),
        Arc::clone(&events_queue),
    )
    .await?;

    let solana_listener = SolanaListener::new(
        solana_stream_client,
        Arc::new(solana_signature_model),
        Arc::clone(&events_queue),
    )
    .await?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:subscriber".to_owned(), redis_conn);

    let gas_service_account = Pubkey::from_str(&config.solana_multisig)?;
    let gateway_account = Pubkey::from_str(&config.solana_gateway)?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = solana_poller.run(gas_service_account, gateway_account) => {},
        _ = solana_listener.run(gas_service_account, gateway_account) => {},
    }

    events_queue.close().await;

    Ok(())
}
