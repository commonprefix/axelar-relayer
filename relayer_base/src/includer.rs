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
use crate::gmp_api::gmp_types::{ExecuteTaskFields, RefundTaskFields};
use crate::payload_cache::PayloadCacheTrait;

pub trait RefundManager {
    type Wallet;
    fn is_refund_manager_managed(&self) -> bool;

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

#[derive(PartialEq, Debug)]
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
    fn broadcast_refund_message(&self, refund_task: RefundTaskFields) -> impl Future<Output = Result<String, BroadcasterError>>;
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
                let maybe_task = serde_json::from_slice::<QueueItem>(&data);

                let task = match maybe_task {
                    Ok(task) => task,
                    Err(e) => {
                        error!("Failed to parse task: {:?}", e);
                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            error!("Failed to ack message: {:?}", e);
                        }
                        return;
                    }
                };

                let consume_res = self.consume(task).await;
                match consume_res {
                    Ok(_) => {
                        info!("Successfully consumed delivery");
                        if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                            error!("Failed to ack message: {:?}", e);
                        }
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
        if let Ok(mut consumer) = queue.consumer().await {
            loop {
                info!("Includer is alive.");
                self.work(&mut consumer, Arc::clone(&queue)).await;
            }
        } else {
            error!("Failed to create consumer");
        }
    }

    pub async fn consume(&self, task: QueueItem) -> Result<(), IncluderError> {
        match task {
            QueueItem::Task(task) => match *task {
                Task::Execute(execute_task) => {
                    info!("Consuming execute task: {:?}", execute_task);
                    let broadcast_result = match self
                        .broadcaster
                        .broadcast_execute_message(execute_task.task)
                        .await
                    {
                        Ok(result) => result,
                        Err(BroadcasterError::IrrelevantTask(_)) => {
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(IncluderError::ConsumerError(e.to_string()));
                        }
                    };

                    if Self::broadcast_result_has_message(&broadcast_result)
                        && broadcast_result.status.is_ok()
                    {
                        return Ok(());
                    }

                    let (gmp_error, retry, err) = match &broadcast_result.status {
                        Err(err) => {
                            let (gmp_error, retry) = match err {
                                BroadcasterError::InsufficientGas(_) => (
                                    crate::gmp_api::gmp_types::CannotExecuteMessageReason::InsufficientGas,
                                    false,
                                ),
                                _ => {
                                    warn!("Failed to broadcast execute message: {:?}", err);
                                    (
                                        crate::gmp_api::gmp_types::CannotExecuteMessageReason::Error,
                                        true,
                                    )
                                }
                            };
                            (gmp_error, retry, err)
                        }
                        Ok(_) => unreachable!("Expected broadcast_result.status to be Err"),
                    };

                    if Self::broadcast_result_has_message(&broadcast_result) {
                        let message_id = broadcast_result.message_id.ok_or_else(|| {
                            IncluderError::ConsumerError("Message ID is missing".to_string())
                        })?;
                        let source_chain = broadcast_result.source_chain.ok_or_else(|| {
                            IncluderError::ConsumerError("Source chain is missing".to_string())
                        })?;

                        self.gmp_api
                            .cannot_execute_message(
                                execute_task.common.id,
                                message_id,
                                source_chain,
                                err.to_string(),
                                gmp_error
                            )
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                    }
                    
                    if !retry {
                        return Ok(());
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
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?,
                        ))
                        .await
                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;

                    info!(
                        "Broadcasting transaction with hash: {:?}",
                        broadcast_result.tx_hash
                    );

                    if Self::broadcast_result_has_message(&broadcast_result)
                    {
                        let message_id = broadcast_result.message_id.ok_or_else(|| {
                            IncluderError::ConsumerError("Message ID is missing".to_string())
                        })?;
                        let source_chain = broadcast_result.source_chain.ok_or_else(|| {
                            IncluderError::ConsumerError("Source chain is missing".to_string())
                        })?;

                        if let Err(e) = broadcast_result.status {
                            // Retry creating proof for this message
                            let cross_chain_id =
                                CrossChainId::new(source_chain.as_str(), message_id.as_str())
                                    .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                            self.construct_proof_queue
                                .publish(QueueItem::RetryConstructProof(cross_chain_id.to_string()))
                                .await;

                            let mut redis_conn = self
                                .redis_pool
                                .get()
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
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
                                    e.to_string(),
                                    crate::gmp_api::gmp_types::CannotExecuteMessageReason::Error
                                )
                                .await
                                .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                        } else {
                            // clear payload from cache, won't be needed anymore
                            self.payload_cache
                                .clear(
                                    CrossChainId::new(source_chain.as_str(), message_id.as_str())
                                        .map_err(|e| IncluderError::ConsumerError(e.to_string()))?,
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
                    if !self.refund_manager.is_refund_manager_managed() {
                        return match self
                            .broadcaster
                            .broadcast_refund_message(refund_task.task)
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(BroadcasterError::InsufficientGas(_)) => Ok(()),
                            Err(e) => Err(IncluderError::GenericError(e.to_string())),
                        }
                    }

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
                        let tx_hash = match self
                            .broadcaster
                            .broadcast_refund(tx_blob)
                            .await
                            .map_err(|e| IncluderError::ConsumerError(e.to_string()))
                        {
                            Ok(hash) => hash, // bind the successful tx_hash here…
                            Err(err) => {
                                // …or on error, release the lock and return
                                self.refund_manager
                                    .release_wallet_lock(wallet)
                                    .map_err(|e| IncluderError::ConsumerError(e.to_string()))?;
                                return Err(err);
                            }
                        };

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
                        if let Err(e) = gas_refunded_post {
                            // TODO: should retry somehow
                            warn!("Failed to post event: {:?}", e);
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

    fn broadcast_result_has_message(broadcast_result: &BroadcastResult<<B as Broadcaster>::Transaction>) -> bool {
        broadcast_result.message_id.is_some()
            && broadcast_result.source_chain.is_some()
    }
}
