use dotenv::dotenv;
use relayer_base::{
    config::Config,
    database::PostgresDB,
    price_feed::PriceFeeder,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let price_feeder = PriceFeeder::new(&config, db).await.unwrap();

    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    setup_heartbeat("heartbeat:price_feed".to_owned(), redis_pool);

    price_feeder.run().await.unwrap();
}
