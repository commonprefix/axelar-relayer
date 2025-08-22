use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::error::SubscriberError;
use relayer_base::redis::connection_manager;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use std::str::FromStr;
use tokio::signal::unix::{signal, SignalKind};
use ton::check_accounts::check_accounts;
use ton::client::TONRpcClient;
use ton::config::TONConfig;
use tonlib_core::TonAddress;

const MIN_BALANCE: u64 = 10_000_000_000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:price_feed".to_owned(), redis_conn, None);

    let mut our_addresses = vec![];
    for wallet in config.wallets {
        our_addresses.push(TonAddress::from_str(&wallet.address)?);
    }
    let gateway = TonAddress::from_str(&config.ton_gateway)?;
    let gas_service = TonAddress::from_str(&config.ton_gas_service)?;
    our_addresses.push(gateway.clone());
    our_addresses.push(gas_service.clone());

    let client = TONRpcClient::new(config.ton_rpc.clone(), config.ton_api_key.clone(), 5, 5, 30)
        .await
        .map_err(|e| error_stack::report!(SubscriberError::GenericError(e.to_string())))
        .expect("Failed to create RPC client");

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = check_accounts(&client, our_addresses, MIN_BALANCE, true) => {}
    }

    Ok(())
}
