use std::sync::Arc;

use redis::{Commands, ExistenceCheck, SetExpiry, SetOptions};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};
use xrpl_api::Transaction;

use relayer_base::{
    gmp_api::gmp_types::{Task, VerifyTask},
    queue::{Queue, QueueItem},
    subscriber::ChainTransaction,
};

use crate::xrpl_transaction::PgXrplTransactionModel;

const CACHE_EXPIRATION: u64 = 60 * 5;

pub struct VotingRetrier {
    events_queue: Arc<Queue>,
    tasks_queue: Arc<Queue>,
    xrpl_transaction: PgXrplTransactionModel,
    redis_pool: r2d2::Pool<redis::Client>,
}

impl VotingRetrier {
    pub fn new(
        events_queue: Arc<Queue>,
        tasks_queue: Arc<Queue>,
        xrpl_transaction: PgXrplTransactionModel,
        redis_pool: r2d2::Pool<redis::Client>,
    ) -> Self {
        Self {
            events_queue,
            tasks_queue,
            xrpl_transaction,
            redis_pool,
        }
    }

    pub async fn process_expired_polls(&self) -> Result<(), anyhow::Error> {
        let mut redis_conn = self.redis_pool.get()?;
        let set_opts = SetOptions::default()
            .conditional_set(ExistenceCheck::NX)
            .with_expiration(SetExpiry::EX(CACHE_EXPIRATION));

        let txs = self.xrpl_transaction.find_expired_events().await.unwrap();
        for entry in txs {
            let tx_available_for_processing: Result<bool, anyhow::Error> = redis_conn
                .set_options(format!("voting_retrier_{}", entry.tx_hash), true, set_opts)
                .map_err(|e| anyhow::anyhow!("Failed to set redis key: {}", e));

            if tx_available_for_processing.is_err() || !tx_available_for_processing.unwrap() {
                // either the key exists in redis which means the tx is already in process,
                // or for some reason the key was not set (e.g. redis is down)
                debug!("Skipping transaction: {}", entry.tx_hash);
                continue;
            }

            let maybe_tx = serde_json::from_str::<Transaction>(&entry.tx).map_err(|e| {
                warn!("Failed to parse transaction: {}", e);
                anyhow::anyhow!("Failed to parse transaction: {}", e)
            });
            if let Ok(tx) = maybe_tx {
                info!("Retrying transaction: {}", entry.tx_hash);
                match entry.message_type.as_str() {
                    "proof" | "add_gas" | "add_reserves" => {
                        let event = QueueItem::Transaction(ChainTransaction::Xrpl(tx));
                        self.events_queue.publish(event.clone()).await;
                        debug!("Published event: {:?}", event);
                    }
                    "interchain_transfer" | "call_contract" => {
                        let verify_task_str = if let Some(verify_task) = entry.verify_task {
                            verify_task
                        } else {
                            warn!("No verify task found for transaction: {}", entry.tx_hash);
                            continue;
                        };

                        let task = if let Ok(verify_task) =
                            serde_json::from_str::<VerifyTask>(&verify_task_str)
                        {
                            QueueItem::Task(Task::Verify(verify_task))
                        } else {
                            warn!("Failed to parse verify task: {}", verify_task_str);
                            continue;
                        };
                        self.tasks_queue.publish(task.clone()).await;
                        debug!("Published task: {:?}", task);
                    }
                    _ => {
                        warn!("Invalid message type: {}", entry.message_type);
                    }
                }
            } else {
                warn!("Failed to parse transaction: {}", entry.tx);
            }
        }

        Ok(())
    }

    pub async fn work(&self) {
        loop {
            self.process_expired_polls().await.unwrap();
            sleep(Duration::from_secs(1)).await;
        }
    }
}
