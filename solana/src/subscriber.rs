use std::sync::Arc;

use crate::client::SolanaClientTrait;
use anyhow::anyhow;
use relayer_base::{
    database::Database,
    error::SubscriberError,
    subscriber::{ChainTransaction, TransactionPoller},
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use solana_types::solana_types::SolanaTransaction;
use tracing::error;

pub struct SolanaSubscriber<DB: Database, X: SolanaClientTrait> {
    client: X,
    last_signature_checked: Option<Signature>,
    db: DB,
    context: String,
}

impl<DB: Database, X: SolanaClientTrait> SolanaSubscriber<DB, X> {
    pub async fn new(client: X, db: DB, context: String) -> Result<Self, SubscriberError> {
        Ok(SolanaSubscriber {
            client,
            last_signature_checked: None,
            db,
            context,
        })
    }

    pub async fn store_last_signature_checked(&mut self) -> Result<(), anyhow::Error> {
        // self.db
        //     .store_latest_height("solana", &self.context, self.last_signature_checked)
        //     .await
        //     .map_err(|e| anyhow!("Error storing latest ledger: {:?}", e))
        Ok(())
    }
}

impl<DB: Database, X: SolanaClientTrait> TransactionPoller for SolanaSubscriber<DB, X> {
    type Transaction = SolanaTransaction;
    type Account = Pubkey;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::Solana(Box::new(tx))
    }

    async fn poll_account(
        &mut self,
        account_id: Pubkey,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let transactions = self
            .client
            .get_transactions_for_account(&account_id, self.last_signature_checked, None)
            .await?;

        let maybe_transaction_with_max_slot = transactions.iter().max_by_key(|tx| tx.slot);
        // if maybe_transaction_with_max_slot.is_some() {
        //     self.last_signature_checked = Some(maybe_transaction_with_max_slot.unwrap().signature);
        //     if let Err(err) = self.store_last_signature_checked().await {
        //         error!("{:?}", err);
        //     }
        // }

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

        // Ok(transactions)
        Err(anyhow!("Not implemented"))
    }

    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        Err(anyhow!("Not implemented"))
    }
}
