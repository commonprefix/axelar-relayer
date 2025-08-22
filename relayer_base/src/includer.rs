use async_trait::async_trait;
use base64::{prelude::BASE64_STANDARD, Engine};
use lapin::message::Delivery;
use lapin::options::BasicAckOptions;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use router_api::CrossChainId;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

use crate::gmp_api::gmp_types::{ExecuteTaskFields, RefundTaskFields};
use crate::gmp_api::GmpApiTrait;
use crate::payload_cache::PayloadCacheTrait;
use crate::queue_consumer::QueueConsumer;
use crate::utils::ThreadSafe;
use crate::{
    database::Database,
    error::{BroadcasterError, IncluderError, RefundManagerError},
    gmp_api::gmp_types::{Amount, CommonEventFields, Event, RefundTask, Task},
    payload_cache::PayloadCache,
    queue::{Queue, QueueItem},
};

#[async_trait]
pub trait RefundManager
where
    Self::Wallet: Send,
{
    type Wallet;
    fn is_refund_manager_managed(&self) -> bool;

    async fn build_refund_tx(
        &self,
        recipient: String,
        amount: String,
        refund_id: &str,
        wallet: &Self::Wallet,
    ) -> Result<Option<(String, String, String)>, RefundManagerError>;
    async fn is_refund_processed(
        &self,
        refund_task: &RefundTask,
        refund_id: &str,
    ) -> Result<bool, RefundManagerError>;
    async fn get_wallet_lock(&self) -> Result<Self::Wallet, RefundManagerError>;
    async fn release_wallet_lock(&self, wallet: Self::Wallet) -> Result<(), RefundManagerError>;
}

#[derive(PartialEq, Debug)]
pub struct BroadcastResult<T> {
    pub transaction: T,
    pub tx_hash: String,
    pub status: Result<(), BroadcasterError>,
    pub message_id: Option<String>,
    pub source_chain: Option<String>,
}

#[async_trait]
pub trait Broadcaster
where
    Self::Transaction: ThreadSafe,
{
    type Transaction;

    async fn broadcast_prover_message(
        &self,
        tx_blob: String,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError>;

    async fn broadcast_refund(&self, tx_blob: String) -> Result<String, BroadcasterError>;
    async fn broadcast_execute_message(
        &self,
        message: ExecuteTaskFields,
    ) -> Result<BroadcastResult<Self::Transaction>, BroadcasterError>;
    async fn broadcast_refund_message(
        &self,
        refund_task: RefundTaskFields,
    ) -> Result<String, BroadcasterError>;
}

pub struct Includer<B, C, R, DB, G>
where
    C: ThreadSafe,
    B: Broadcaster + ThreadSafe,
    R: RefundManager + ThreadSafe,
    DB: Database + ThreadSafe,
    G: GmpApiTrait + ThreadSafe,
{
    pub chain_client: C,
    pub broadcaster: B,
    pub refund_manager: R,
    pub gmp_api: Arc<G>,
    pub payload_cache: PayloadCache<DB>,
    pub construct_proof_queue: Arc<Queue>,
    pub redis_conn: ConnectionManager,
}

#[async_trait]
impl<B, C, R, DB, G> QueueConsumer for Includer<B, C, R, DB, G>
where
    C: ThreadSafe,
    B: Broadcaster + ThreadSafe,
    R: RefundManager + ThreadSafe,
    DB: Database + ThreadSafe,
    G: GmpApiTrait + ThreadSafe,
{
    async fn on_delivery(&self, delivery: Delivery, queue: Arc<Queue>, tracker: &TaskTracker) {
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
}

impl<B, C, R, DB, G> Includer<B, C, R, DB, G>
where
    C: ThreadSafe,
    B: Broadcaster + ThreadSafe,
    R: RefundManager + ThreadSafe,
    DB: Database + ThreadSafe,
    G: GmpApiTrait + ThreadSafe,
{
    pub async fn run(&self, queue: Arc<Queue>, token: CancellationToken) {
        if let Ok(mut consumer) = queue.consumer().await {
            loop {
                info!("Includer is alive.");
                self.work(&mut consumer, Arc::clone(&queue), token.clone())
                    .await;
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
                                gmp_error,
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

                    if Self::broadcast_result_has_message(&broadcast_result) {
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

                            let mut redis_conn = self.redis_conn.clone();
                            let redis_key = format!("failed_proof:{}", cross_chain_id);
                            let _: i64 = redis_conn
                                .incr(redis_key.clone(), 1)
                                .await
                                .map_err(|e| IncluderError::GenericError(e.to_string()))?;
                            redis_conn
                                .expire::<_, ()>(redis_key.clone(), 60 * 60 * 12)
                                .await // 12 hours
                                .map_err(|e| IncluderError::GenericError(e.to_string()))?;

                            self.gmp_api
                                .cannot_execute_message(
                                    gateway_tx_task.common.id,
                                    message_id,
                                    source_chain,
                                    e.to_string(),
                                    crate::gmp_api::gmp_types::CannotExecuteMessageReason::Error,
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
                        };
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
                        .await
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
                                    .await
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
                        .await
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

    fn broadcast_result_has_message(
        broadcast_result: &BroadcastResult<<B as Broadcaster>::Transaction>,
    ) -> bool {
        broadcast_result.message_id.is_some() && broadcast_result.source_chain.is_some()
    }
}
