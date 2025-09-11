use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::anyhow;
use lapin::{
    message::Delivery,
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions,
        BasicQosOptions, ConfirmSelectOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions,
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
    task::JoinHandle,
    time::{self, Duration},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn, Instrument, Span};
use uuid::Uuid;

use crate::logging::distributed_tracing_headers;
use crate::{gmp_api::gmp_types::Task, subscriber::ChainTransaction};

const DEAD_LETTER_EXCHANGE: &str = "dlx_exchange";
const DEAD_LETTER_QUEUE_PREFIX: &str = "dead_letter_";
const MAX_RETRIES: u16 = 3;
const BUFFER_SIZE: usize = 1000;

#[derive(Clone)]
pub struct Queue {
    url: String,
    name: String,
    channel: Arc<Mutex<lapin::Channel>>,
    queue: Arc<RwLock<lapin::Queue>>,
    retry_queue: Arc<RwLock<lapin::Queue>>,
    buffer_sender: Arc<Sender<QueueItemWithSpan>>,
    buffer_processor: Arc<RwLock<Option<BufferProcessor>>>,
    num_workers: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum QueueItem {
    Task(Box<Task>),
    Transaction(Box<ChainTransaction>),
    RetryConstructProof(String),
}

pub struct QueueItemWithSpan {
    pub item: QueueItem,
    pub span: Span,
}

struct BufferProcessor {
    buffer_sender: Arc<Sender<QueueItemWithSpan>>,
    buffer_receiver: Arc<Mutex<Receiver<QueueItemWithSpan>>>,
    queue: Arc<Queue>,
    handle: Option<JoinHandle<()>>,
    shutdown_signal: Arc<AtomicBool>,
    cancellation_token: CancellationToken,
}

impl BufferProcessor {
    fn new(
        buffer_sender: Arc<Sender<QueueItemWithSpan>>,
        buffer_receiver: Receiver<QueueItemWithSpan>,
        queue: Arc<Queue>,
    ) -> Self {
        Self {
            buffer_sender,
            buffer_receiver: Arc::new(Mutex::new(buffer_receiver)),
            queue,
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            handle: None,
            cancellation_token: CancellationToken::new(),
        }
    }

    fn run(&mut self) {
        let receiver_mutex = Arc::clone(&self.buffer_receiver);
        let sender = Arc::clone(&self.buffer_sender);
        let queue = Arc::clone(&self.queue);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);
        let cancellation_token = self.cancellation_token.clone();
        self.handle = Some(tokio::spawn(async move {
            loop {
                let mut buffer_receiver = receiver_mutex.lock().await;

                tokio::select! {
                    receipt = buffer_receiver.recv() => {
                        if let Some(queue_item_with_span) = receipt {
                            let item = &queue_item_with_span.item;
                            let span = queue_item_with_span.span.clone();
                            if let Err(e) = queue.publish_item(item, false, None).instrument(span).await {
                                error!("Failed to publish item: {:?}. Re-buffering.", e);
                                if let Err(e) = sender.send(queue_item_with_span).await {
                                    error!("Failed to re-buffer item: {:?}", e);
                                }
                            }
                        }
                    },
                    _ = cancellation_token.cancelled() => {
                        warn!("Buffer processor forced to cancel");
                        break;
                    }
                }

                if shutdown_signal.load(Ordering::Acquire) {
                    if buffer_receiver.capacity() < BUFFER_SIZE {
                        info!(
                            "Emptying {} item(s) from buffer",
                            BUFFER_SIZE - buffer_receiver.capacity()
                        );
                        continue;
                    }
                    info!("Shutting down buffer processor");
                    drop(buffer_receiver);
                    break;
                }
            }
        }));
    }

    pub async fn shutdown(&self) {
        self.shutdown_signal.store(true, Ordering::Release);
    }
}

impl Queue {
    pub async fn new(url: &str, name: &str, num_workers: u16) -> Arc<Self> {
        let (_, channel, queue, retry_queue) = Self::connect(url, name, num_workers).await;

        let (buffer_sender, buffer_receiver) = mpsc::channel::<QueueItemWithSpan>(BUFFER_SIZE);

        let queue_arc = Arc::new(Self {
            url: url.to_owned(),
            name: name.to_owned(),
            channel: Arc::new(Mutex::new(channel)),
            queue: Arc::new(RwLock::new(queue)),
            retry_queue: Arc::new(RwLock::new(retry_queue)),
            buffer_sender: Arc::new(buffer_sender),
            buffer_processor: Arc::new(RwLock::new(None)),
            num_workers,
        });

        let mut processor = BufferProcessor::new(
            Arc::clone(&queue_arc.buffer_sender),
            buffer_receiver,
            Arc::clone(&queue_arc),
        );
        processor.run();
        *queue_arc.buffer_processor.write().await = Some(processor);

        let queue_clone = Arc::clone(&queue_arc);
        tokio::spawn(async move {
            queue_clone.connection_health_check().await;
        });

        queue_arc
    }

    #[tracing::instrument(skip(self))]
    pub async fn republish(
        &self,
        delivery: Delivery,
        force_requeue: bool,
    ) -> Result<(), anyhow::Error> {
        let data = delivery.data.clone();
        let item: QueueItem = serde_json::from_slice(&data)?;

        if force_requeue {
            if let Err(ack_err) = delivery.ack(BasicAckOptions { multiple: false }).await {
                return Err(anyhow!("Failed to ack message: {:?}", ack_err));
            }
            self.publish(item).await; // publish at the tail of the queue
            return Ok(());
        }

        let properties = delivery.properties.clone();
        let retry_count = properties
            .headers()
            .as_ref()
            .and_then(|headers| headers.inner().get("x-retry-count"))
            .and_then(|count| count.as_short_uint())
            .unwrap_or(0);

        if retry_count > MAX_RETRIES {
            debug!("Exceeded max retries, nacking message: {:?}", item);
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
            debug!("Republishing message: {:?}", item);
            let mut new_headers = properties
                .headers()
                .clone()
                .unwrap_or(FieldTable::from(BTreeMap::new()))
                .inner()
                .clone();
            new_headers.insert(
                ShortString::from("x-retry-count"),
                AMQPValue::ShortUInt(retry_count + 1),
            );
            let properties = properties.with_headers(FieldTable::from(new_headers));

            if let Err(e) = self.publish_item(&item, true, Some(properties)).await {
                delivery
                    .nack(BasicNackOptions {
                        multiple: false,
                        requeue: true,
                    })
                    .await
                    .ok(); // best effort
                return Err(anyhow!("Failed to republish item: {:?}", e));
            }

            if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                return Err(anyhow!("Failed to ack message: {:?}", e));
            }
        }

        Ok(())
    }

    async fn connection_health_check(&self) {
        let mut interval = time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !self.is_connected().await {
                        warn!("Connection with RabbitMQ failed. Reconnecting.");
                        self.refresh_connection().await;
                    }
                },
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
        num_workers: u16,
    ) -> Result<(Channel, lapin::Queue, lapin::Queue), Box<dyn std::error::Error>> {
        // Create channel
        let channel = connection.create_channel().await?;

        // Enable confirmations
        channel
            .confirm_select(ConfirmSelectOptions { nowait: false })
            .await?;

        channel
            .basic_qos(num_workers, BasicQosOptions::default())
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

    async fn connect(
        url: &str,
        name: &str,
        num_workers: u16,
    ) -> (Connection, Channel, lapin::Queue, lapin::Queue) {
        loop {
            match Connection::connect(url, ConnectionProperties::default()).await {
                Ok(connection) => {
                    info!("Connected to RabbitMQ at {}", url);
                    let setup_result = Queue::setup_rabbitmq(&connection, name, num_workers).await;
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

    pub async fn refresh_connection(&self) {
        info!("Reconnecting to RabbitMQ at {}", self.url);
        let (_, new_channel, new_queue, new_retry_queue) =
            Self::connect(&self.url, &self.name, self.num_workers).await;

        let mut channel_lock = self.channel.lock().await;
        *channel_lock = new_channel;

        let mut queue_lock = self.queue.write().await;
        *queue_lock = new_queue;

        let mut retry_queue_lock = self.retry_queue.write().await;
        *retry_queue_lock = new_retry_queue;

        info!("Reconnected to RabbitMQ at {}", self.url);
    }

    #[tracing::instrument(skip(self))]
    pub async fn publish(&self, item: QueueItem) {
        let span = Span::current();
        let queue_item_with_span = QueueItemWithSpan {
            item,
            span: span.clone(),
        };
        if let Err(e) = self.buffer_sender.send(queue_item_with_span).await {
            error!("Buffer is full, failed to enqueue message: {:?}", e);
        }
    }

    #[tracing::instrument(skip(self))]
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

        let properties = properties.unwrap_or(BasicProperties::default().with_delivery_mode(2));
        let headers = distributed_tracing_headers(&Span::current());
        let properties = properties.with_headers(FieldTable::from(headers));

        let confirm = channel_lock
            .basic_publish(
                "",
                queue_lock.name().as_str(),
                BasicPublishOptions::default(),
                &msg,
                properties,
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

    pub async fn close(&self) {
        info!("Shutting down {} queue gracefully…", self.name);
        let buffer_processor = self.buffer_processor.write().await.take();
        if let Some(processor) = buffer_processor {
            processor.shutdown().await;
            let mut ticker = time::interval(Duration::from_secs(5));
            ticker.tick().await; // throw away the immediate tick

            if let Some(handle) = processor.handle {
                tokio::select! {
                    _ = handle => {
                        info!("Buffer processor closed");
                    }
                    _ = ticker.tick() => {
                        warn!("Force closing buffer processor after 5 seconds");
                        processor.cancellation_token.cancel();
                    }
                }
            } else {
                warn!("No handle found for buffer processor");
            }
        }
    }
}
