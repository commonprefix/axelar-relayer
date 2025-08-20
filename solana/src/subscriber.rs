use std::sync::Arc;

use crate::client::SolanaClientTrait;
use anyhow::anyhow;
use relayer_base::{
    database::Database,
    error::SubscriberError,
    subscriber::{ChainTransaction, TransactionPoller},
};
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use tracing::warn;

pub struct SolanaSubscriber<DB: Database, X: SolanaClientTrait> {
    client: X,
    latest_ledger: i64,
    db: DB,
    context: String,
}

impl<DB: Database, X: SolanaClientTrait> SolanaSubscriber<DB, X> {
    pub async fn new(client: X, db: DB, context: String) -> Result<Self, SubscriberError> {
        let latest_ledger = 0;
        Ok(SolanaSubscriber {
            client,
            latest_ledger,
            db,
            context,
        })
    }
}

impl<DB: Database, X: SolanaClientTrait> TransactionPoller for SolanaSubscriber<DB, X> {
    type Transaction = EncodedConfirmedTransactionWithStatusMeta;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Arc::new(tx))
    }

    async fn poll_account(
        &mut self,
        account_id: Pubkey,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        // let transactions = self
        //     .client
        //     .get_transactions_for_account(&account_id, (self.latest_ledger + 1) as u32)
        //     .await?;

        // let max_response_ledger = transactions
        //     .iter()
        //     .map(|tx| tx.common().ledger_index.unwrap_or(0))
        //     .max();
        // if max_response_ledger.is_some() {
        //     self.latest_ledger = max_response_ledger
        //         .ok_or(anyhow!("Max response ledger is None"))?
        //         .into();
        //     if let Err(err) = self.store_latest_ledger().await {
        //         warn!("{:?}", err);
        //     }
        // }
        Err(anyhow!("Not implemented"))
    }

    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        Err(anyhow!("Not implemented"))
    }
}
