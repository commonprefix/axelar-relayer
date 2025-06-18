use anyhow::Result;
use dotenv::dotenv;
use futures::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use relayer_base::{config::Config, utils::setup_logging};
use std::env;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
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
    info!("connected to {}", &config.queue_address);

    channel
        .queue_declare(
            dst_queue,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    info!("destination queue: {}", dst_queue);

    let mut consumer = channel
        .basic_consume(
            src_queue,
            "migrator",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    info!("consuming from source queue: {}", src_queue);

    while let Some(delivery_result) = consumer.next().await {
        match delivery_result {
            Ok(delivery) => {
                let payload = delivery.data.clone();

                let maybe_publish = channel
                    .basic_publish(
                        "",
                        dst_queue,
                        BasicPublishOptions::default(),
                        &payload,
                        BasicProperties::default().with_delivery_mode(2),
                    )
                    .await;

                match maybe_publish {
                    Ok(confirm) => {
                        let _ = confirm.await;
                        delivery.ack(BasicAckOptions::default()).await?;
                        info!("moved one message from {} to {}", src_queue, dst_queue);
                    }
                    Err(e) => {
                        error!("failed to publish to {}: {}", dst_queue, e);
                        delivery
                            .nack(BasicNackOptions {
                                multiple: false,
                                requeue: true,
                            })
                            .await?;
                    }
                }
            }
            Err(e) => {
                error!("consumer error on {}: {:?}", src_queue, e);
            }
        }
    }

    Ok(())
}
