use crate::queue::Queue;
use async_std::stream::StreamExt;
use async_trait::async_trait;
use lapin::message::Delivery;
use lapin::Consumer;
use std::sync::Arc;
use tokio::select;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

#[async_trait]
pub trait QueueConsumer {
    async fn on_delivery(&self, delivery: Delivery, queue: Arc<Queue>, tracker: &TaskTracker);

    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>, token: CancellationToken) {
        let tracker = TaskTracker::new();

        loop {
            debug!("Task tracker size: {}", tracker.len());
            info!("Waiting for messages from {}", consumer.queue());
            select! {
                _ = token.cancelled() => {
                    info!("Cancellation requested; no longer awaiting consumer.next()");
                    break;
                }
                maybe_msg = consumer.next() => {
                    match maybe_msg {
                        Some(Ok(delivery)) => {
                            self.on_delivery(delivery, Arc::clone(&queue), &tracker).await;
                        }
                        Some(Err(e)) => {
                            error!("Failed to receive delivery: {:?}", e);
                        }
                        None => {
                            //TODO:  Consumer stream ended. Possibly handle reconnection logic here if needed.
                            warn!("No more messages from consumer.");
                        }
                    }
                }
            }
        }

        info!("Task tracker size: {}", tracker.len());
        tracker.close();
        tracker.wait().await;
    }
}
