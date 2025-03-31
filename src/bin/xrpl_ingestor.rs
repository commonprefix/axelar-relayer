use dotenv::dotenv;
use std::sync::Arc;

use axelar_xrpl_relayer::{
    config::NetworkConfig,
    gmp_api,
    ingestor::Ingestor,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.ingestor.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let events_queue = Queue::new(&config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config).unwrap());

    let ingestor = Ingestor::new(gmp_api.clone(), config.clone());
    ingestor.run(events_queue, tasks_queue).await;
}
