use dotenv::dotenv;
use relayer_base::{
    config::Config, database::PostgresDB, price_feed::PriceFeeder, utils::setup_logging,
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let price_feeder = PriceFeeder::new(&config, db).await.unwrap();
    price_feeder.run().await.unwrap();
}
