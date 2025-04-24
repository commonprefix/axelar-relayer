use axelar_relayer::{config::NetworkConfig, price_feed::PriceFeeder, utils::setup_logging};
use dotenv::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = NetworkConfig::from_yaml("config.yaml", &network).unwrap();

    let _guard = setup_logging(&config);
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let price_feeder = PriceFeeder::new(&config, redis_pool).await.unwrap();
    price_feeder.run().await.unwrap();
}
