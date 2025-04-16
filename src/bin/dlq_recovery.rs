use anyhow::Result;
use futures::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions,
        ConfirmSelectOptions,
    },
    types::FieldTable,
    Connection, ConnectionProperties,
};
use std::env;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let amqp_url = env::var("AMQP_URL").expect("AMQP_URL is not set");
    let main_queue = env::var("MAIN_QUEUE").expect("MAIN_QUEUE is not set");
    let dlq_name = format!("dead_letter_{}", main_queue);

    info!("Connecting to RabbitMQ at {}", amqp_url);

    // Connect to RabbitMQ.
    let connection = Connection::connect(&amqp_url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    channel
        .confirm_select(ConfirmSelectOptions { nowait: false })
        .await?;

    // Create a consumer on the DLQ.
    let consumer = channel
        .basic_consume(
            &dlq_name,
            "dlq_republisher",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    info!(
        "Connected. Processing messages from DLQ '{}' to main queue '{}'",
        dlq_name, main_queue
    );

    // Create a BufReader for asynchronous standard input.
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();

    // Process messages from the DLQ.
    tokio::pin!(consumer);
    while let Some(result) = consumer.next().await {
        match result {
            Ok(delivery) => {
                let data_str = String::from_utf8_lossy(&delivery.data);
                info!("Message received from DLQ:\n{}\n", data_str);
                info!("Republish this message? (Y/N): ");

                // Wait for user input asynchronously.
                let mut user_input = String::new();
                if let Ok(Some(line)) = reader.next_line().await {
                    user_input = line;
                }

                // Check the user's response.
                if user_input.trim().eq_ignore_ascii_case("y") {
                    let mut new_headers = delivery
                        .properties
                        .headers()
                        .clone()
                        .expect("Headers are not set")
                        .inner()
                        .clone();
                    new_headers.remove("x-retry-count"); // reset the retry count
                    let new_properties = delivery
                        .properties
                        .clone()
                        .with_headers(FieldTable::from(new_headers))
                        .with_delivery_mode(2);

                    // Republishing to the main queue using the default exchange.
                    match channel
                        .basic_publish(
                            "",
                            &main_queue,
                            BasicPublishOptions::default(),
                            &delivery.data,
                            new_properties,
                        )
                        .await
                    {
                        Ok(confirm) => {
                            // Wait for the broker confirmation.
                            if confirm.await?.is_ack() {
                                delivery.ack(BasicAckOptions::default()).await?;
                                info!("Republished message");
                            } else {
                                error!("Message not acknowledged by broker.");
                                delivery
                                    .nack(BasicNackOptions {
                                        multiple: false,
                                        requeue: true,
                                    })
                                    .await?;
                            }
                        }
                        Err(e) => {
                            error!("Failed to publish message: {:?}", e);
                        }
                    }
                } else {
                    delivery
                        .nack(BasicNackOptions {
                            multiple: false,
                            requeue: true,
                        })
                        .await?;
                }
            }
            Err(e) => {
                error!("Error receiving from consumer: {:?}", e);
            }
        }
    }

    info!("Done processing DLQ messages.");
    Ok(())
}
