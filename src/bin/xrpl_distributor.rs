use dotenv::dotenv;
use std::sync::Arc;

use axelar_xrpl_relayer::{
    config::NetworkConfig,
    distributor::Distributor,
    gmp_api,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.distributor.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    let mut distributor = Distributor::new(redis_pool.clone());
    distributor.run(gmp_api, tasks_queue).await;
}
