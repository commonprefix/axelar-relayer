use base64::{prelude::BASE64_STANDARD, Engine};
use futures::StreamExt;
use lapin::{options::BasicAckOptions, Consumer};
use redis::Commands;
use router_api::CrossChainId;
use std::{future::Future, sync::Arc};
use tracing::{debug, error, info, warn};

use crate::{
    database::Database,
    error::{BroadcasterError, IncluderError, RefundManagerError},
    gmp_api::{
        gmp_types::{Amount, CommonEventFields, Event, RefundTask, Task},
        GmpApi,
    },
    payload_cache::PayloadCache,
    queue::{Queue, QueueItem},
};
use crate::gmp_api::gmp_types::ExecuteTaskFields;

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
    fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields
    ) -> impl Future<Output = Result<BroadcastResult<Self::Transaction>, BroadcasterError>>;
}

pub struct Includer<B, C, R, DB>
where
    B: Broadcaster,
    R: RefundManager,
    DB: Database,
{
    pub chain_client: C,
    pub broadcaster: B,
    pub refund_manager: R,
    pub gmp_api: Arc<GmpApi>,
    pub payload_cache: PayloadCache<DB>,
    pub construct_proof_queue: Arc<Queue>,
    pub redis_pool: r2d2::Pool<redis::Client>,
}

impl<B, C, R, DB> Includer<B, C, R, DB>
where
    B: Broadcaster,
    R: RefundManager,
    DB: Database,
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
                // We probably want to clean up this file, and maybe even move consume logic
                // up to chain includer
                Task::Execute(execute_task) => {
                    info!("Consuming task: {:?}", execute_task);
                    let broadcast_result = self
                        .broadcaster
                        .broadcast_execute_message(execute_task.task)
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;


                    if broadcast_result.message_id.is_some()
                        && broadcast_result.source_chain.is_some() && broadcast_result.status.is_ok()
                    {
                        return Ok(());
                    }

                    let err = broadcast_result.status.unwrap_err();

                    if broadcast_result.message_id.is_some()
                        && broadcast_result.source_chain.is_some() {
                        let message_id = broadcast_result.message_id.unwrap();
                        let source_chain = broadcast_result.source_chain.unwrap();

                        self.gmp_api
                            .cannot_execute_message(
                                execute_task.common.id,
                                message_id,
                                source_chain,
                                err.to_string(),
                            )
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    }

                    Err(IncluderError::ConsumerError(err.to_string()))
                }
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

                    info!(
                        "Broadcasting transaction with hash: {:?}",
                        broadcast_result.tx_hash
                    );
                    if broadcast_result.message_id.is_some()
                        && broadcast_result.source_chain.is_some()
                    {
                        let message_id = broadcast_result.message_id.unwrap();
                        let source_chain = broadcast_result.source_chain.unwrap();

                        if broadcast_result.status.is_err() {
                            // Retry creating proof for this message
                            let cross_chain_id =
                                CrossChainId::new(source_chain.as_str(), message_id.as_str())
                                    .unwrap();
                            self.construct_proof_queue
                                .publish(QueueItem::RetryConstructProof(cross_chain_id.to_string()))
                                .await;

                            let mut redis_conn = self.redis_pool.get().unwrap();
                            let redis_key = format!("failed_proof:{}", cross_chain_id);
                            let _: i64 = redis_conn
                                .incr(redis_key.clone(), 1)
                                .map_err(|e| IncluderError::GenericError(e.to_string()))?;
                            redis_conn
                                .expire::<_, ()>(redis_key.clone(), 60 * 60 * 12) // 12 hours
                                .map_err(|e| IncluderError::GenericError(e.to_string()))?;

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
