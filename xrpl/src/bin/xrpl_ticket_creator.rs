use dotenv::dotenv;
use std::sync::Arc;

use relayer_base::config::config_from_yaml;
use relayer_base::{
    gmp_api,
    utils::{setup_heartbeat, setup_logging},
};
use xrpl::config::XRPLConfig;
use xrpl::ticket_creator::XrplTicketCreator;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    setup_heartbeat("heartbeat:ticket_creator".to_owned(), redis_pool);

    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, false).unwrap());
    let ticket_creator = XrplTicketCreator::new(gmp_api.clone(), config.clone());
    ticket_creator.run().await;
}
