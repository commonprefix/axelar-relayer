use anyhow::anyhow;
use lapin::{
    options::{BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Connection, ConnectionProperties, Consumer,
};
use tracing::info;

#[derive(Clone)]
pub struct Queue {
    queue: lapin::Queue,
    channel: lapin::Channel,
}

impl Queue {
    pub async fn new(url: &str, name: &str) -> Self {
        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .unwrap();
        info!("Connected to RabbitMQ at {}", url);

        // Create channel
        let channel = connection.create_channel().await.unwrap();
        info!("Created channel");

        // Create Q
        // todo: test durable
        let q = channel
            .queue_declare(name, QueueDeclareOptions::default(), FieldTable::default())
            .await
            .unwrap();
        info!("Declared RMQ queue: {:?}", q);

        Self { queue: q, channel }
    }

    pub async fn publish(&self, msg: &[u8]) {
        let confirm = self
            .channel
            .basic_publish(
                "",
                &self.queue.name().as_str(),
                BasicPublishOptions::default(),
                msg,
                BasicProperties::default(),
            )
            .await
            .unwrap();
        info!("Published message: {:?}", confirm);
    }

    pub async fn consumer(&self) -> Result<Consumer, anyhow::Error> {
        Ok(self
            .channel
            .basic_consume(
                self.queue.name().as_str(),
                "my consumer",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow!("Failed to create consumer: {:?}", e))?)
    }
}