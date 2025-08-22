use dotenv::dotenv;

use xrpl::{funder::XRPLFunder, XRPLClient};

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use xrpl::config::XRPLConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:funder".to_owned(), redis_conn, None);

    let xrpl_client = XRPLClient::new(&config.xrpl_rpc, 3)?;
    let funder = XRPLFunder::new(config, xrpl_client);
    funder.run().await;

    Ok(())
}
