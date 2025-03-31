use dotenv::dotenv;

use axelar_xrpl_relayer::{
    config::NetworkConfig,
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use xrpl_types::AccountId;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.subscriber.clone());

    let events_queue = Queue::new(&config.queue_address, "events").await;
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    let account = AccountId::from_address(&config.xrpl_multisig).unwrap();

    let mut subscriber = Subscriber::new_xrpl(&config.xrpl_rpc, redis_pool.clone()).await;
    subscriber.run(account.to_address(), events_queue).await;
}
