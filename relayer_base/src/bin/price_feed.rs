use dotenv::dotenv;
use relayer_base::{
    database::PostgresDB,
    price_feed::PriceFeeder,
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let db = PostgresDB::new(&config.postgres_url).await.unwrap();
    let price_feeder = PriceFeeder::new(&config, db).await.unwrap();

    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_conn = connection_manager(redis_client, None, None, None)
        .await
        .expect("Failed to get redis connection");
    setup_heartbeat("heartbeat:price_feed".to_owned(), redis_conn);

    price_feeder.run().await.unwrap();
}
