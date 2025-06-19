use anyhow::Result;
use dotenv::dotenv;
use futures::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use relayer_base::{config::Config, utils::setup_logging};
use std::env;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let network = env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network))?;
    let _guard = setup_logging(&config);

    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        error!("Usage: {} <src_queue> <dst_queue>", args[0]);
        std::process::exit(1);
    }
    let src_queue = &args[1];
    let dst_queue = &args[2];

    let conn = Connection::connect(&config.queue_address, ConnectionProperties::default()).await?;
    let channel: Channel = conn.create_channel().await?;
    info!("Connected to {}", &config.queue_address);

    let mut consumer = channel
        .basic_consume(
            src_queue,
            "migrator",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("Moving messages: from '{}' to '{}'", src_queue, dst_queue);

    while let Some(maybe_delivery) = consumer.next().await {
        match maybe_delivery {
            Ok(delivery) => {
                let payload = delivery.data.clone();

                if let Err(e) = channel
                    .basic_publish(
                        "",
                        dst_queue,
                        BasicPublishOptions::default(),
                        &payload,
                        BasicProperties::default().with_delivery_mode(2), // persistent
                    )
                    .await
                {
                    error!("Publish to '{}' failed: {}", dst_queue, e);
                    continue;
                }

                if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                    error!("ACK of source queue failed: {}", e);
                } else {
                    info!("Moved one message");
                }
            }
            Err(e) => {
                error!("Error reading from '{}': {:?}", src_queue, e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }

    Ok(())
}
