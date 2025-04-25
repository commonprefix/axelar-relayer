use dotenv::dotenv;
use std::sync::Arc;

use axelar_relayer::{
    config::Config,
    distributor::{Distributor, RecoverySettings},
    gmp_api::{self, gmp_types::TaskKind},
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.distributor.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();

    let mut distributor = Distributor::new_with_recovery_settings(
        redis_pool.clone(),
        "task_recovery".to_string(),
        RecoverySettings {
            from_task_id: Some("".to_string()),
            to_task_id: "".to_string(),
            tasks_filter: Some(vec![]),
        },
    )
    .await;
    distributor.run_recovery(gmp_api, tasks_queue).await;
}
