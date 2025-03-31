use dotenv::dotenv;
use std::sync::Arc;

use axelar_xrpl_relayer::{
    config::NetworkConfig,
    gmp_api,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
    xrpl::XrplIncluder,
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.includer.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config).unwrap());
    let xrpl_includer = XrplIncluder::new(config.clone(), gmp_api.clone())
        .await
        .unwrap();

    xrpl_includer.run(tasks_queue).await;
}
