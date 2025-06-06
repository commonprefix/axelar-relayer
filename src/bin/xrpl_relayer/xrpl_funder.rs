use dotenv::dotenv;

use axelar_relayer::{config::Config, utils::setup_logging, xrpl::XRPLFunder};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let funder = XRPLFunder::new(config);
    funder.run().await;
}
