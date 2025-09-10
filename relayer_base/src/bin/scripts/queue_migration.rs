use anyhow::Result;
use dotenv::dotenv;
use futures::StreamExt;
use lapin::options::BasicAckOptions;
use relayer_base::config::config_from_yaml;
use relayer_base::logging::setup_logging;
use relayer_base::{
    gmp_api::gmp_types::TaskKind,
    queue::{Queue, QueueItem},
};
use std::env;
use std::sync::Arc;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let network = env::var("NETWORK").expect("NETWORK must be set");
    let config = config_from_yaml(&format!("config.{}.yaml", network))?;
    let (_sentry_guard, otel_guard) = setup_logging(&config);

    //let conn = Connection::connect(&config.queue_address, ConnectionProperties::default()).await?;
    //let channel: Channel = conn.create_channel().await?;
    info!("Connected to {}", &config.queue_address);

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let includer_tasks_queue = Queue::new(&config.queue_address, "includer_tasks").await;
    let ingestor_tasks_queue = Queue::new(&config.queue_address, "ingestor_tasks").await;

    let mut consumer = tasks_queue.consumer().await?;
    info!(
        "Moving messages: from 'tasks' queue to 'includer_tasks' and 'ingestor_tasks' accordingly"
    );

    while let Some(maybe_delivery) = consumer.next().await {
        match maybe_delivery {
            Ok(delivery) => {
                let data = delivery.data.clone();
                let task_item: QueueItem = serde_json::from_slice::<QueueItem>(&data)?;
                let task = match task_item.clone() {
                    QueueItem::Task(task) => task,
                    _ => continue,
                };
                info!("Publishing task: {:?}", task);
                let queue = match task.kind() {
                    TaskKind::Refund | TaskKind::GatewayTx => Arc::clone(&includer_tasks_queue),
                    TaskKind::Verify
                    | TaskKind::ConstructProof
                    | TaskKind::ReactToWasmEvent
                    | TaskKind::ReactToRetriablePoll
                    | TaskKind::ReactToExpiredSigningSession => Arc::clone(&ingestor_tasks_queue),
                    TaskKind::Unknown | TaskKind::Execute => {
                        warn!("Dropping and acking unknown task: {:?}", task);
                        delivery.ack(BasicAckOptions::default()).await?;
                        continue;
                    }
                };
                queue.publish(task_item.clone()).await;

                if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                    error!("ACK of source queue failed: {}", e);
                } else {
                    info!("Moved one entry to the appropriate queue");
                }
            }
            Err(e) => {
                error!("Error reading from 'tasks_queue': {:?}", e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }

    otel_guard
        .force_flush()
        .expect("Failed to flush OTEL messages");

    Ok(())
}
