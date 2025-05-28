use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use relayer_base::{
    config::Config,
    database::PostgresDB,
    distributor::{Distributor, RecoverySettings},
    gmp_api::{self, gmp_types::TaskKind},
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.distributor.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();

    let mut distributor = Distributor::new_with_recovery_settings(
        postgres_db,
        "task_recovery".to_string(),
        gmp_api,
        RecoverySettings {
            from_task_id: Some("01968759-7d7f-7ccc-a5d0-d03539d725bb".to_string()),
            to_task_id: "01968759-b2eb-72d3-93da-4d7384617be0".to_string(),
            tasks_filter: Some(vec![TaskKind::GatewayTx]),
            // from_task_id: Some("01968759-4ebe-746a-a992-c63f6f7c2f1d".to_string()),
            // to_task_id: "01968759-51c7-7367-a8d0-7f12f2e0efdb".to_string(),
            // tasks_filter: Some(vec![TaskKind::ConstructProof]),
        },
        config.refunds_enabled,
    )
    .await;

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = distributor.run_recovery(tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;

    Ok(())
}
