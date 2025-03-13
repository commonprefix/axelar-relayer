use std::sync::Arc;

use anyhow::anyhow;
use lapin::{
    message::Delivery,
    options::{
        BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, ConfirmSelectOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    BasicProperties, Connection, ConnectionProperties, Consumer,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex, RwLock,
    },
    time::{self, Duration},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{gmp_api::gmp_types::Task, subscriber::ChainTransaction};

#[derive(Clone)]
pub struct Queue {
    channel: Arc<Mutex<lapin::Channel>>,
    queue: Arc<RwLock<lapin::Queue>>,
    buffer_sender: Sender<QueueItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum QueueItem {
    Task(Task),
    Transaction(ChainTransaction),
}

impl Queue {
    pub async fn new(url: &str, name: &str) -> Arc<Self> {
        let (_, channel, queue) = Self::connect(url, name).await;

        let (buffer_sender, buffer_receiver) = mpsc::channel::<QueueItem>(1000);

        let queue_arc = Arc::new(Self {
            channel: Arc::new(Mutex::new(channel)),
            queue: Arc::new(RwLock::new(queue)),
            buffer_sender,
        });

        let queue_clone = queue_arc.clone();
        let url = url.to_owned();
        let name = name.to_owned();
        tokio::spawn(async move {
            queue_clone
                .run_buffer_processor(buffer_receiver, url, name)
                .await;
        });

        queue_arc
    }

    pub async fn republish(&self, delivery: Delivery) -> Result<(), anyhow::Error> {
        let item: QueueItem = serde_json::from_slice(&delivery.data)?;
        let mut nack_options = BasicNackOptions {
            multiple: false,
            requeue: false,
        };

        if let Err(e) = self.publish_item(&item).await {
            warn!("Failed to republish item: {:?}", e);
            nack_options.requeue = true;
        } else {
            nack_options.requeue = false;
        }

        if let Err(nack_err) = delivery.nack(nack_options).await {
            return Err(anyhow!("Failed to nack message: {:?}", nack_err)); // This should really not happen
        }

        Ok(())
    }

    async fn run_buffer_processor(
        &self,
        mut buffer_receiver: Receiver<QueueItem>,
        url: String,
        name: String,
    ) {
        let mut interval = time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                Some(item) = buffer_receiver.recv() => {
                    if let Err(e) = self.publish_item(&item).await {
                        error!("Failed to publish item: {:?}. Re-buffering.", e);
                        if let Err(e) = self.buffer_sender.send(item).await {
                            error!("Failed to re-buffer item: {:?}", e);
                        }
                    }
                },
                _ = interval.tick() => {
                    if !self.is_connected().await {
                        warn!("Connection with RabbitMQ failed. Reconnecting.");
                        self.refresh_connection(&url, &name).await;
                    }
                },
                else => {
                    break;
                }
            }
        }
    }

    async fn is_connected(&self) -> bool {
        let channel_lock = self.channel.lock().await;
        channel_lock.status().connected()
    }

    async fn connect(url: &str, name: &str) -> (Connection, lapin::Channel, lapin::Queue) {
        loop {
            match Connection::connect(url, ConnectionProperties::default()).await {
                Ok(connection) => {
                    info!("Connected to RabbitMQ at {}", url);
                    match connection.create_channel().await {
                        Ok(channel) => {
                            info!("Created channel");

                            if let Err(e) = channel
                                .confirm_select(ConfirmSelectOptions { nowait: false })
                                .await
                            {
                                error!("Failed to send confirm_select to RMQ: {:?}", e);
                            } else {
                                match channel
                                    .queue_declare(
                                        name,
                                        QueueDeclareOptions::default(),
                                        FieldTable::default(),
                                    )
                                    .await
                                {
                                    Ok(queue) => {
                                        info!("Declared RMQ queue: {:?}", queue);
                                        return (connection, channel, queue);
                                    }
                                    Err(e) => {
                                        error!("Failed to declare queue: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to create channel: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to connect to RabbitMQ: {:?}. Retrying in 5 seconds...",
                        e
                    );
                }
            }
            time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn refresh_connection(&self, url: &str, name: &str) {
        info!("Reconnecting to RabbitMQ at {}", url);
        let (_, new_channel, new_queue) = Self::connect(url, name).await;

        let mut channel_lock = self.channel.lock().await;
        *channel_lock = new_channel;

        let mut queue_lock = self.queue.write().await;
        *queue_lock = new_queue;

        info!("Reconnected to RabbitMQ at {}", url);
    }

    pub async fn publish(&self, item: QueueItem) {
        if let Err(e) = self.buffer_sender.send(item).await {
            error!("Buffer is full, failed to enqueue message: {:?}", e);
        }
    }

    async fn publish_item(&self, item: &QueueItem) -> Result<(), anyhow::Error> {
        let msg = serde_json::to_vec(item)?;

        let channel_lock = self.channel.lock().await;
        let queue_lock = self.queue.read().await;

        let confirm = channel_lock
            .basic_publish(
                "",
                queue_lock.name().as_str(),
                BasicPublishOptions::default(),
                &msg,
                BasicProperties::default(),
            )
            .await?
            .await?;

        if confirm.is_ack() {
            Ok(())
        } else {
            Err(anyhow!("Failed to publish message"))
        }
    }

    pub async fn consumer(&self) -> Result<Consumer, anyhow::Error> {
        let consumer_tag = format!("consumer_{}", Uuid::new_v4());

        let channel_lock = self.channel.lock().await;
        let queue_lock = self.queue.read().await;

        Ok(channel_lock
            .basic_consume(
                queue_lock.name().as_str(),
                &consumer_tag,
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow!("Failed to create consumer: {:?}", e))?)
    }
}
