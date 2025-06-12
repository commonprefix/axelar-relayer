use dotenv::dotenv;

use xrpl::funder::XRPLFunder;

use relayer_base::{
    config::Config,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    setup_heartbeat(config.heartbeats.funder.clone(), redis_pool);

    let funder = XRPLFunder::new(config);
    funder.run().await;
}
