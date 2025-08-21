use std::sync::Arc;

use crate::{client::SolanaClientTrait, models::solana_subscriber_cursor::SubscriberCursor};
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

pub struct SolanaSubscriber<X: SolanaClientTrait, SC: SubscriberCursor> {
    client: X,
    last_signature_checked: Option<Signature>,
    cursor_model: SC,
    context: String,
}

impl<X: SolanaClientTrait, SC: SubscriberCursor> SolanaSubscriber<X, SC> {
    pub async fn new(
        client: X,
        context: String,
        cursor_model: SC,
    ) -> Result<Self, SubscriberError> {
        Ok(SolanaSubscriber {
            client,
            last_signature_checked: None,
            cursor_model,
            context,
        })
    }

    pub async fn store_last_signature_checked(&mut self) -> Result<(), anyhow::Error> {
        match self.last_signature_checked {
            Some(signature) => self
                .cursor_model
                .store_latest_signature(&self.context, signature.to_string())
                .await
                .map_err(|e| anyhow!("Error storing latest signature: {:?}", e)),
            None => Err(anyhow!("No signature to store")),
        }
    }
}

impl<X: SolanaClientTrait, SC: SubscriberCursor> TransactionPoller for SolanaSubscriber<X, SC> {
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
            .get_transactions_for_account(&account_id, None, self.last_signature_checked)
            .await?;

        let maybe_transaction_with_max_slot = transactions.iter().max_by_key(|tx| tx.slot);
        if let Some(transaction) = maybe_transaction_with_max_slot {
            self.last_signature_checked = Some(transaction.signature);
            if let Err(err) = self.store_last_signature_checked().await {
                error!("{:?}", err);
                return Err(err);
            }
        }
        Ok(transactions)
    }

    async fn poll_tx(&mut self, tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        Err(anyhow!("Not implemented"))
    }
}
