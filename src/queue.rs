use std::{collections::BTreeMap, sync::Arc};

use anyhow::anyhow;
use lapin::{
    message::Delivery,
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions,
        ConfirmSelectOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
    },
    types::{AMQPValue, FieldTable, ShortString},
    BasicProperties, Channel, Connection, ConnectionProperties, Consumer, ExchangeKind,
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

const DEAD_LETTER_EXCHANGE: &str = "dlx_exchange";
const DEAD_LETTER_QUEUE_PREFIX: &str = "dead_letter_";
const MAX_RETRIES: u16 = 3;

#[derive(Clone)]
pub struct Queue {
    channel: Arc<Mutex<lapin::Channel>>,
    queue: Arc<RwLock<lapin::Queue>>,
    retry_queue: Arc<RwLock<lapin::Queue>>,
    buffer_sender: Sender<QueueItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum QueueItem {
    Task(Task),
    Transaction(ChainTransaction),
}

impl Queue {
    pub async fn new(url: &str, name: &str) -> Arc<Self> {
        let (_, channel, queue, retry_queue) = Self::connect(url, name).await;

        let (buffer_sender, buffer_receiver) = mpsc::channel::<QueueItem>(1000);

        let queue_arc = Arc::new(Self {
            channel: Arc::new(Mutex::new(channel)),
            queue: Arc::new(RwLock::new(queue)),
            retry_queue: Arc::new(RwLock::new(retry_queue)),
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

        let retry_count = delivery
            .properties
            .headers()
            .as_ref()
            .and_then(|headers| headers.inner().get("x-retry-count"))
            .and_then(|count| count.as_short_uint())
            .unwrap_or(0);

        if retry_count >= MAX_RETRIES {
            if let Err(nack_err) = delivery
                .nack(BasicNackOptions {
                    multiple: false,
                    requeue: false,
                })
                .await
            {
                return Err(anyhow!("Failed to nack message: {:?}", nack_err)); // This should really not happen
            }
        } else {
            let mut new_headers = BTreeMap::new();
            new_headers.insert(
                ShortString::from("x-retry-count"),
                AMQPValue::ShortUInt(retry_count + 1),
            );
            let properties = delivery
                .properties
                .clone()
                .with_headers(FieldTable::from(new_headers));

            if let Err(e) = self.publish_item(&item, true, Some(properties)).await {
                return Err(anyhow!("Failed to republish item: {:?}", e));
            }

            if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                return Err(anyhow!("Failed to ack message: {:?}", e));
            }
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
                    if let Err(e) = self.publish_item(&item, false, None).await {
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

    async fn setup_rabbitmq(
        connection: &Connection,
        name: &str,
    ) -> Result<(Channel, lapin::Queue, lapin::Queue), Box<dyn std::error::Error>> {
        // Create channel
        let channel = connection.create_channel().await?;

        // Enable confirmations
        channel
            .confirm_select(ConfirmSelectOptions { nowait: false })
            .await?;

        // Declare DLX
        channel
            .exchange_declare(
                DEAD_LETTER_EXCHANGE, // DLX name
                ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        // Declare DLQ
        let dlq_name = format!("{}{}", DEAD_LETTER_QUEUE_PREFIX, name);
        channel
            .queue_declare(
                &dlq_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        // Bind DLQ to DLX
        channel
            .queue_bind(
                &dlq_name,
                DEAD_LETTER_EXCHANGE,
                &dlq_name,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        // Dead-lettering for retry queue -- puts messages back to the main queue after TTL expires
        let mut retry_args = FieldTable::default();
        retry_args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString("".into()),
        );
        retry_args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString(name.into()),
        );
        retry_args.insert("x-message-ttl".into(), AMQPValue::LongUInt(10000));

        // Declare retry queue
        let retry_queue = channel
            .queue_declare(
                &format!("retry_{}", name),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                retry_args,
            )
            .await?;

        // Dead-lettering for main queue
        let mut args = FieldTable::default();
        args.insert(
            "x-dead-letter-exchange".into(),
            AMQPValue::LongString(DEAD_LETTER_EXCHANGE.into()),
        );
        args.insert(
            "x-dead-letter-routing-key".into(),
            AMQPValue::LongString(dlq_name.into()),
        );

        // Declare main queue
        let queue = channel
            .queue_declare(
                name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                args,
            )
            .await?;

        Ok((channel, queue, retry_queue))
    }

    async fn connect(url: &str, name: &str) -> (Connection, Channel, lapin::Queue, lapin::Queue) {
        loop {
            match Connection::connect(url, ConnectionProperties::default()).await {
                Ok(connection) => {
                    info!("Connected to RabbitMQ at {}", url);
                    let setup_result = Queue::setup_rabbitmq(&connection, name).await;
                    if let Ok((channel, queue, retry_queue)) = setup_result {
                        return (connection, channel, queue, retry_queue);
                    } else {
                        error!("Failed to setup RabbitMQ: {:?}", setup_result.err());
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
        let (_, new_channel, new_queue, new_retry_queue) = Self::connect(url, name).await;

        let mut channel_lock = self.channel.lock().await;
        *channel_lock = new_channel;

        let mut queue_lock = self.queue.write().await;
        *queue_lock = new_queue;

        let mut retry_queue_lock = self.retry_queue.write().await;
        *retry_queue_lock = new_retry_queue;

        info!("Reconnected to RabbitMQ at {}", url);
    }

    pub async fn publish(&self, item: QueueItem) {
        if let Err(e) = self.buffer_sender.send(item).await {
            error!("Buffer is full, failed to enqueue message: {:?}", e);
        }
    }

    async fn publish_item(
        &self,
        item: &QueueItem,
        retry_queue: bool,
        properties: Option<BasicProperties>,
    ) -> Result<(), anyhow::Error> {
        let msg = serde_json::to_vec(item)?;

        let channel_lock = self.channel.lock().await;
        let queue_lock = if retry_queue {
            self.retry_queue.read().await
        } else {
            self.queue.read().await
        };

        let confirm = channel_lock
            .basic_publish(
                "",
                queue_lock.name().as_str(),
                BasicPublishOptions::default(),
                &msg,
                properties.unwrap_or(BasicProperties::default().with_delivery_mode(2)),
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

        channel_lock
            .basic_consume(
                queue_lock.name().as_str(),
                &consumer_tag,
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| anyhow!("Failed to create consumer: {:?}", e))
    }
}
