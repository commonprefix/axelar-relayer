use base64::{prelude::BASE64_STANDARD, Engine};
use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use std::{future::Future, sync::Arc};
use tracing::{debug, error, info, warn};

use crate::{
    error::{BroadcasterError, IncluderError, RefundManagerError},
    gmp_api::{
        gmp_types::{Amount, CannotExecuteMessageReason, CommonEventFields, Event, Task},
        GmpApi,
    },
    queue::{Queue, QueueItem},
};

pub trait RefundManager {
    fn build_refund_tx(
        &self,
        recipient: String,
        amount: String,
    ) -> impl Future<Output = Result<Option<(String, String, String)>, RefundManagerError>>;
}

pub trait Broadcaster {
    fn broadcast(
        &self,
        tx_blob: String,
    ) -> impl Future<Output = Result<(Result<String, BroadcasterError>, String, String), BroadcasterError>>;
}

pub struct Includer<B, C, R>
where
    B: Broadcaster,
    R: RefundManager,
{
    pub chain_client: C,
    pub broadcaster: B,
    pub refund_manager: R,
    pub gmp_api: Arc<GmpApi>,
}

impl<B, C, R> Includer<B, C, R>
where
    B: Broadcaster,
    R: RefundManager,
{
    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>) -> () {
        match consumer.next().await {
            Some(Ok(delivery)) => {
                let data = delivery.data.clone();
                let task = serde_json::from_slice::<QueueItem>(&data).unwrap();

                let consume_res = self.consume(task).await;
                match consume_res {
                    Ok(_) => {
                        info!("Successfully consumed delivery");
                        delivery.ack(BasicAckOptions::default()).await.expect("ack");
                    }
                    Err(e) => {
                        match e {
                            IncluderError::IrrelevantTask => {
                                debug!("Skipping irrelevant task");
                            }
                            _ => {
                                error!("Failed to consume delivery: {:?}", e);
                            }
                        }

                        if let Err(nack_err) = queue.republish(delivery).await {
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
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await
    }

    pub async fn run(&self, queue: Arc<Queue>) -> () {
        let mut consumer = queue.consumer().await.unwrap();
        loop {
            info!("Includer is alive.");
            self.work(&mut consumer, queue.clone()).await;
        }
    }

    pub async fn consume(&self, task: QueueItem) -> Result<(), IncluderError> {
        match task {
            QueueItem::Task(task) => match task {
                Task::GatewayTx(gateway_tx_task) => {
                    info!("Consuming task: {:?}", gateway_tx_task);
                    let (tx_result, message_id, source_chain) = self
                        .broadcaster
                        .broadcast(hex::encode(
                            BASE64_STANDARD
                                .decode(gateway_tx_task.task.execute_data)
                                .unwrap(),
                        ))
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    if tx_result.is_err() {
                        let cannot_execute_message_event = Event::CannotExecuteMessageV2 {
                            common: CommonEventFields {
                                r#type: "CANNOT_EXECUTE_MESSAGE/V2".to_owned(),
                                event_id: format!(
                                    "cannot-execute-task-v{}-{}",
                                    2, gateway_tx_task.common.id
                                ),
                                meta: None,
                            },
                            message_id,
                            source_chain,
                            reason: CannotExecuteMessageReason::Error, // TODO
                            details: tx_result.unwrap_err().to_string(),
                        };

                        self.gmp_api
                            .post_events(vec![cannot_execute_message_event])
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                    }
                    Ok(())
                }
                Task::Refund(refund_task) => {
                    info!("Consuming task: {:?}", refund_task);
                    if refund_task.task.remaining_gas_balance.token_id.is_some() {
                        return Err(IncluderError::GenericError(
                            "Refund task with token_id is not supported".to_string(),
                        ));
                    }
                    let refund_info = self
                        .refund_manager
                        .build_refund_tx(
                            refund_task.task.refund_recipient_address.clone(),
                            refund_task.task.remaining_gas_balance.amount, // TODO: check if this is correct
                        )
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    if let Some((tx_blob, refunded_amount, fee)) = refund_info {
                        let (tx_result, _, _) = self
                            .broadcaster
                            .broadcast(tx_blob)
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                        if tx_result.is_err() {
                            return Err(IncluderError::ConsumerError(
                                tx_result.unwrap_err().to_string(),
                            ));
                        }

                        let gas_refunded = Event::GasRefunded {
                            common: CommonEventFields {
                                r#type: "GAS_REFUNDED".to_owned(),
                                event_id: tx_result.unwrap(),
                                meta: None,
                            },
                            recipient_address: refund_task.task.refund_recipient_address,
                            refunded_amount: Amount {
                                token_id: None,
                                amount: refunded_amount,
                            },
                            cost: Amount {
                                token_id: None,
                                amount: fee,
                            },
                        };

                        let gas_refunded_post = self.gmp_api.post_events(vec![gas_refunded]).await;
                        if gas_refunded_post.is_err() {
                            // TODO: should retry somehow
                            warn!("Failed to post event: {:?}", gas_refunded_post.unwrap_err());
                        }
                    } else {
                        warn!("Refund not executed: refund amount is not enough to cover tx fees");
                    }
                    Ok(())
                }
                _ => Err(IncluderError::IrrelevantTask),
            },
            _ => Err(IncluderError::GenericError(
                "Invalid queue item".to_string(),
            )),
        }
    }
}
