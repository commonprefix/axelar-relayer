use dotenv::dotenv;
use sqlx::PgPool;

use relayer_base::{
    gmp_api,
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::config::{config_from_yaml};
use relayer_base::redis::connection_manager;
use xrpl::config::XRPLConfig;
use xrpl::ticket_creator::XrplTicketCreator;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone()).unwrap();
    let redis_conn = connection_manager(redis_client, None, None, None)
        .await
        .unwrap();

    setup_heartbeat("heartbeat:ticket_creator".to_owned(), redis_conn);

    let pg_pool = PgPool::connect(&config.common_config.postgres_url)
        .await
        .unwrap();
    let gmp_api = gmp_api::construct_gmp_api(pg_pool, &config.common_config, false).unwrap();

    let ticket_creator = XrplTicketCreator::new(gmp_api.clone(), config.clone());
    ticket_creator.run().await;
}
