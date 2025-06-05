use dotenv::dotenv;
use std::sync::Arc;

use relayer_base::{config::Config, gmp_api, utils::setup_logging};

use xrpl::ticket_creator::XrplTicketCreator;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, false).unwrap());
    let ticket_creator = XrplTicketCreator::new(gmp_api.clone(), config.clone());
    ticket_creator.run().await;
}
