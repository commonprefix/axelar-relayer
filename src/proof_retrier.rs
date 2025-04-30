use std::sync::Arc;

use async_std::stream::StreamExt;
use lapin::{message::Delivery, options::BasicAckOptions};
use router_api::CrossChainId;
use tracing::{debug, error, info, warn};

use crate::{
    gmp_api::gmp_types::{CommonTaskFields, ConstructProofTask, ConstructProofTaskFields, Task},
    payload_cache::PayloadCache,
    queue::{Queue, QueueItem},
};

pub struct ProofRetrier {
    pub payload_cache: PayloadCache,
    pub construct_proof_queue: Arc<Queue>,
    pub tasks_queue: Arc<Queue>,
}

impl ProofRetrier {
    pub fn new(
        payload_cache: PayloadCache,
        construct_proof_queue: Arc<Queue>,
        tasks_queue: Arc<Queue>,
    ) -> Self {
        Self {
            payload_cache,
            construct_proof_queue,
            tasks_queue,
        }
    }

    pub async fn process_delivery(&self, delivery: &Delivery) -> Result<(), anyhow::Error> {
        let item: QueueItem = serde_json::from_slice(&delivery.data)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize QueueItem: {}", e))?;
        let cc_id = match item {
            QueueItem::RetryConstructProof(cc_id) => cc_id,
            _ => {
                return Err(anyhow::anyhow!("Irrelevant queue item"));
            }
        };

        let source_chain = cc_id.split('_').next().unwrap();
        let message_id = cc_id.split('_').nth(1).unwrap();
        let cc_id = CrossChainId::new(source_chain, message_id)
            .map_err(|e| anyhow::anyhow!("Failed to parse CrossChainId: {}", e))?;
        let payload_cache_value = self
            .payload_cache
            .get(cc_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get payload: {}", e))?;

        if let Some(payload_cache_value) = payload_cache_value {
            let construct_proof_task = Task::ConstructProof(ConstructProofTask {
                common: CommonTaskFields {
                    id: "TODO".to_string(),
                    chain: "xrpl".to_string(), // TODO: don't hardcode this
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    r#type: "CONSTRUCT_PROOF".to_string(),
                    meta: None,
                },
                task: ConstructProofTaskFields {
                    message: payload_cache_value.message,
                    payload: payload_cache_value.payload,
                },
            });

            self.tasks_queue
                .publish(QueueItem::Task(construct_proof_task.clone()))
                .await;
            info!("Published construct proof task: {:?}", construct_proof_task);
        } else {
            return Err(anyhow::anyhow!(
                "Payload not found in cache for CrossChainId: {}",
                cc_id.to_string()
            ));
        }

        Ok(())
    }

    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let mut consumer = self
            .construct_proof_queue
            .consumer()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create consumer: {}", e))?;

        loop {
            match consumer.next().await {
                Some(Ok(delivery)) => {
                    let item = serde_json::from_slice::<QueueItem>(&delivery.data).unwrap();
                    match item {
                        QueueItem::RetryConstructProof(_) => {
                            match self.process_delivery(&delivery).await {
                                Ok(_) => delivery.ack(BasicAckOptions::default()).await?,
                                Err(e) => {
                                    error!("Failed to process delivery: {:?}", e);
                                    if let Err(nack_err) =
                                        self.construct_proof_queue.republish(delivery, false).await
                                    {
                                        error!("Failed to republish message: {:?}", nack_err);
                                    }
                                }
                            }
                        }
                        _ => {
                            debug!("Skipping irrelevant queue item");
                            if let Err(nack_err) =
                                self.construct_proof_queue.republish(delivery, true).await
                            {
                                error!("Failed to republish message: {:?}", nack_err);
                            }
                        }
                    }
                }
                Some(Err(e)) => {
                    error!("Failed to receive delivery: {:?}", e);
                }
                None => {
                    warn!("No more messages from consumer.");
                }
            }
        }
    }
}
