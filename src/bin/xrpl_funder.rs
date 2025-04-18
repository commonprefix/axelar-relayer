use dotenv::dotenv;

use axelar_relayer::{config::NetworkConfig, utils::{setup_heartbeat, setup_logging}, xrpl::XRPLFunder};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.funder.clone());

    let funder = XRPLFunder::new(config);
    funder.run().await;
}
