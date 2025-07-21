use dotenv::dotenv;

use xrpl::{funder::XRPLFunder, XRPLClient};

use relayer_base::{
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::config::{config_from_yaml};
use xrpl::config::XRPLConfig;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    setup_heartbeat("heartbeat:funder".to_owned(), redis_pool);

    let xrpl_client = XRPLClient::new(&config.xrpl_rpc, 3).unwrap();
    let funder = XRPLFunder::new(config, xrpl_client);
    funder.run().await;
}
