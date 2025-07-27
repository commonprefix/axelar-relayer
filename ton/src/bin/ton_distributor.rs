use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::config::config_from_yaml;
use relayer_base::gmp_api::gmp_types::TaskKind;
use relayer_base::{
    database::PostgresDB,
    distributor::Distributor,
    gmp_api,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};
use relayer_base::gmp_api::GmpApiTrait;
use ton::config::TONConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let includer_tasks_queue =
        Queue::new(&config.common_config.queue_address, "includer_tasks").await;
    let ingestor_tasks_queue =
        Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config.common_config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url)
        .await
        .unwrap();

    let mut distributor = Distributor::new(
        postgres_db,
        "default".to_string(),
        gmp_api,
        config.common_config.refunds_enabled,
    )
    .await;
    distributor.set_supported_includer_tasks(vec![
        TaskKind::Refund,
        TaskKind::Execute,
        TaskKind::GatewayTx,
    ]);

    let redis_client = redis::Client::open(config.common_config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    setup_heartbeat("heartbeat:distributor".to_owned(), redis_pool);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = distributor.run(
            includer_tasks_queue.clone(),
            ingestor_tasks_queue.clone(),
        ) => {},
    }

    ingestor_tasks_queue.close().await;
    includer_tasks_queue.close().await;

    Ok(())
}
