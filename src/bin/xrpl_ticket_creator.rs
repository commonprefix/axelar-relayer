use dotenv::dotenv;
use std::sync::Arc;

use axelar_relayer::{
    config::NetworkConfig,
    gmp_api,
    utils::{setup_heartbeat, setup_logging},
    xrpl::XrplTicketCreator,
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.ticket_creator.clone());

    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, false).unwrap());
    let ticket_creator = XrplTicketCreator::new(gmp_api.clone(), config.clone());
    ticket_creator.run().await;
}
