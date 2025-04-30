use base64::{prelude::BASE64_STANDARD, Engine};
use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use router_api::CrossChainId;
use std::{future::Future, sync::Arc};
use tracing::{debug, error, info, warn};

use crate::{
    error::{BroadcasterError, IncluderError, RefundManagerError},
    gmp_api::{
        gmp_types::{Amount, CommonEventFields, Event, RefundTask, Task},
        GmpApi,
    },
    payload_cache::PayloadCache,
    queue::{Queue, QueueItem},
};

pub trait RefundManager {
    type Wallet;

    fn build_refund_tx(
        &self,
        recipient: String,
        amount: String,
        refund_id: &str,
        wallet: &Self::Wallet,
    ) -> impl Future<Output = Result<Option<(String, String, String)>, RefundManagerError>>;
    fn is_refund_processed(
        &self,
        refund_task: &RefundTask,
        refund_id: &str,
    ) -> impl Future<Output = Result<bool, RefundManagerError>>;
    fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError>;
    fn release_wallet_lock(&self, wallet: Self::Wallet) -> Result<(), RefundManagerError>;
}

pub struct BroadcastResult<T> {
    pub transaction: T,
    pub tx_hash: String,
    pub status: Result<(), BroadcasterError>,
    pub message_id: Option<String>,
    pub source_chain: Option<String>,
}

pub trait Broadcaster {
    type Transaction;

    fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> impl Future<Output = Result<BroadcastResult<Self::Transaction>, BroadcasterError>>;

    fn broadcast_refund(
        &self,
        tx_blob: String,
    ) -> impl Future<Output = Result<String, BroadcasterError>>;
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
    pub payload_cache: PayloadCache,
    pub construct_proof_queue: Arc<Queue>,
}

impl<B, C, R> Includer<B, C, R>
where
    B: Broadcaster,
    R: RefundManager,
{
    async fn work(&self, consumer: &mut Consumer, queue: Arc<Queue>) {
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
                        let mut force_requeue = false;
                        match e {
                            IncluderError::IrrelevantTask => {
                                debug!("Skipping irrelevant task");
                                force_requeue = true;
                            }
                            _ => {
                                error!("Failed to consume delivery: {:?}", e);
                            }
                        }

                        if let Err(nack_err) = queue.republish(delivery, force_requeue).await {
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

    pub async fn run(&self, queue: Arc<Queue>) {
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
                    let broadcast_result = self
                        .broadcaster
                        .broadcast_prover_message(hex::encode(
                            BASE64_STANDARD
                                .decode(gateway_tx_task.task.execute_data)
                                .unwrap(),
                        ))
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    if broadcast_result.message_id.is_some()
                        && broadcast_result.source_chain.is_some()
                    {
                        let message_id = broadcast_result.message_id.unwrap();
                        let source_chain = broadcast_result.source_chain.unwrap();

                        if broadcast_result.status.is_err() {
                            // Retry creating proof for this message
                            self.construct_proof_queue
                                .publish(QueueItem::RetryConstructProof(
                                    CrossChainId::new(source_chain.as_str(), message_id.as_str())
                                        .unwrap()
                                        .to_string(),
                                ))
                                .await;

                            self.gmp_api
                                .cannot_execute_message(
                                    gateway_tx_task.common.id,
                                    message_id,
                                    source_chain,
                                    broadcast_result.status.unwrap_err().to_string(),
                                )
                                .await
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                        } else {
                            // clear payload from cache, won't be needed anymore
                            self.payload_cache
                                .clear(
                                    CrossChainId::new(source_chain.as_str(), message_id.as_str())
                                        .unwrap(),
                                )
                                .await
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                        }
                        return Ok(());
                    }

                    broadcast_result
                        .status
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))
                }
                Task::Refund(refund_task) => {
                    info!("Consuming task: {:?}", refund_task);
                    if self
                        .refund_manager
                        .is_refund_processed(&refund_task, &refund_task.common.id)
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?
                    {
                        warn!("Refund already processed");
                        return Ok(());
                    }

                    if refund_task.task.remaining_gas_balance.token_id.is_some() {
                        return Err(IncluderError::GenericError(
                            "Refund task with token_id is not supported".to_string(),
                        ));
                    }

                    let wallet = self
                        .refund_manager
                        .get_wallet_lock()
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    let refund_info = self
                        .refund_manager
                        .build_refund_tx(
                            refund_task.task.refund_recipient_address.clone(),
                            refund_task.task.remaining_gas_balance.amount, // TODO: check if this is correct
                            &refund_task.common.id,
                            &wallet,
                        )
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    if let Some((tx_blob, refunded_amount, fee)) = refund_info {
                        let broadcast_result = self
                            .broadcaster
                            .broadcast_refund(tx_blob)
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()));

                        if broadcast_result.is_err() {
                            self.refund_manager
                                .release_wallet_lock(wallet)
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                            return Err(broadcast_result.unwrap_err());
                        }
                        let tx_hash = broadcast_result.unwrap();

                        let gas_refunded = Event::GasRefunded {
                            common: CommonEventFields {
                                r#type: "GAS_REFUNDED".to_owned(),
                                event_id: tx_hash,
                                meta: None,
                            },
                            message_id: refund_task.task.message.message_id,
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
                    self.refund_manager
                        .release_wallet_lock(wallet)
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
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
