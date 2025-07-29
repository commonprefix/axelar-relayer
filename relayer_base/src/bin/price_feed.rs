use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::{
    database::PostgresDB,
    price_feed::PriceFeeder,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config);

    let db = PostgresDB::new(&config.postgres_url).await?;
    let price_feeder = PriceFeeder::new(&config, db).await?;

    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:price_feed".to_owned(), redis_pool);

    price_feeder.run().await?;

    Ok(())
}
