use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signature,
    transaction::{Transaction, TransactionError},
};
use sqlx::{PgPool, Type};

use relayer_base::models::Model;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SolanaTransactionStatus {
    /// State when the transaction was successful
    Successful(SolanaTransaction),
    /// State when the transaction was unsuccessful
    Failed {
        /// the raw tx object
        tx: SolanaTransaction,
        /// the actual tx error
        error: TransactionError,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
#[sqlx(type_name = "source_enum", rename_all = "PascalCase")]
pub enum SolanaTransactionSource {
    Prover,
    User,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SolanaTransaction {
    /// signature of the transaction (id)
    pub signature: Signature,
    /// optional timespamp
    pub timestamp: Option<DateTime<Utc>>,
    /// The raw transaction logs
    pub logs: Vec<String>,
    /// The accounts that were passed to an instructoin.
    /// - first item: the program id
    /// - second item: the Pubkeys provided to the ix
    /// - third item: payload data
    pub ixs: Vec<(Pubkey, Vec<Pubkey>, Vec<u8>)>,
    /// the slot number of the tx
    pub slot: u64,
    /// How expensive was the transaction expressed in lamports
    pub cost_in_lamports: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl SolanaTransaction {
    pub fn from_native_transaction(
        tx: &Transaction,
        multisig_address: &str,
    ) -> Result<SolanaTransaction, anyhow::Error> {
        // let common = tx.common();
        // let tx_hash = common
        //     .hash
        //     .clone()
        //     .ok_or(anyhow::anyhow!("No hash found"))?
        //     .to_lowercase();

        // // Calculate created_at from ledger timestamp + ripple epoch
        // let ledger_timestamp = Decimal::from(common.date.ok_or(anyhow::anyhow!("No date found"))?);
        // let ripple_epoch = Decimal::from(946684800);
        // let created_at = chrono::DateTime::from_timestamp(
        //     (ledger_timestamp + ripple_epoch)
        //         .to_string()
        //         .parse::<i64>()?,
        //     0,
        // )
        // .ok_or(anyhow::anyhow!(
        //     "Failed to convert ledger timestamp to chrono datetime"
        // ))?;

        // // Get message type from memos
        // let memos = common.memos.clone();
        // let message_type: Option<XRPLMessageType> = extract_and_decode_memo(&memos, "type")
        //     .ok()
        //     .and_then(|message_type_str| {
        //         serde_json::from_str(&format!("\"{}\"", message_type_str)).ok()
        //     });

        // Ok(XrplTransaction {
        //     tx_hash: tx_hash.clone(),
        //     tx: serde_json::to_string(&tx).unwrap_or_default(),
        //     tx_type: XrplTransactionType::try_from(tx.clone()).unwrap_or_default(),
        //     message_id: Some(tx_hash),
        //     message_type: message_type.map(|message_type| message_type.to_string()),
        //     status: XrplTransactionStatus::Detected,
        //     verify_task: None,
        //     verify_tx: None,
        //     quorum_reached_task: None,
        //     route_tx: None,
        //     source: if tx.common().account == multisig_address {
        //         XrplTransactionSource::Prover
        //     } else {
        //         XrplTransactionSource::User
        //     },
        //     sequence: Some(tx.common().ticket_sequence.unwrap_or(tx.common().sequence) as i64),
        //     created_at,
        // })
        Err(anyhow::anyhow!("Not implemented"))
    }
}

const PG_TABLE_NAME: &str = "solana_transactions";
#[derive(Debug, Clone)]
pub struct PgSolanaTransactionModel {
    pool: PgPool,
}

impl Model<SolanaTransaction, String> for PgSolanaTransactionModel {
    async fn find(&self, id: String) -> Result<Option<SolanaTransaction>> {
        // let query = format!("SELECT * FROM {} WHERE signature = $1", PG_TABLE_NAME);
        // let tx = sqlx::query_as::<_, SolanaTransaction>(&query)
        //     .bind(id)
        //     .fetch_optional(&self.pool)
        //     .await?;
        // Ok(tx)
        Err(anyhow::anyhow!("Not implemented"))
    }

    async fn upsert(&self, tx: SolanaTransaction) -> Result<()> {
        // let query = format!(
        //     "INSERT INTO {} (signature, timestamp, logs, ixs, slot, cost_in_lamports, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (signature) DO UPDATE SET timestamp = $2, logs = $3, ixs = $4, slot = $5, cost_in_lamports = $6, created_at = $7 RETURNING *",
        //     PG_TABLE_NAME
        // );

        // sqlx::query(&query)
        //     .bind(tx.signature)
        //     .bind(tx.timestamp)
        //     .bind(tx.logs)
        //     .bind(tx.ixs)
        //     .bind(tx.slot)
        //     .bind(tx.cost_in_lamports)
        //     .bind(tx.created_at)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    async fn delete(&self, tx: SolanaTransaction) -> Result<()> {
        // let query = format!("DELETE FROM {} WHERE signature = $1", PG_TABLE_NAME);
        // sqlx::query(&query)
        //     .bind(tx.signature)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }
}

impl PgSolanaTransactionModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn update_status(
        &self,
        tx_hash: &str,
        status: SolanaTransactionStatus,
    ) -> Result<()> {
        // let query = format!(
        //     "UPDATE {} SET status = $1 WHERE signature = $2",
        //     PG_TABLE_NAME
        // );

        // sqlx::query(&query)
        //     .bind(status)
        //     .bind(tx_hash)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub async fn update_verify_task(&self, tx_hash: &str, verify_task: &str) -> Result<()> {
        // let query = format!(
        //     "UPDATE {} SET verify_task = $1 WHERE tx_hash = $2",
        //     PG_TABLE_NAME
        // );
        // sqlx::query(&query)
        //     .bind(verify_task)
        //     .bind(tx_hash)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub async fn update_verify_tx(&self, tx_hash: &str, verify_tx: &str) -> Result<()> {
        //  let query = format!(
        //     "UPDATE {} SET verify_tx = $1 WHERE tx_hash = $2",
        //     PG_TABLE_NAME
        // );
        // sqlx::query(&query)
        //     .bind(verify_tx)
        //     .bind(tx_hash)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub async fn update_quorum_reached_task(
        &self,
        tx_hash: &str,
        quorum_reached_task: &str,
    ) -> Result<()> {
        // let query = format!(
        //     "UPDATE {} SET quorum_reached_task = $1 WHERE tx_hash = $2",
        //     PG_TABLE_NAME
        // );
        // sqlx::query(&query)
        //     .bind(quorum_reached_task)
        //     .bind(tx_hash)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub async fn update_route_tx(&self, tx_hash: &str, route_tx: &str) -> Result<()> {
        //  let query = format!(
        //     "UPDATE {} SET route_tx = $1 WHERE tx_hash = $2",
        //     PG_TABLE_NAME
        // );
        // sqlx::query(&query)
        //     .bind(route_tx)
        //     .bind(tx_hash)
        //     .execute(&self.pool)
        //     .await?;

        // Ok(())
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub async fn find_expired_events(&self) -> Result<Vec<SolanaTransaction>> {
        // let query = format!(
        //     "SELECT * FROM {} WHERE quorum_reached_task IS NULL AND verify_tx IS NOT NULL AND created_at < NOW() - INTERVAL '5 minutes'",
        //     PG_TABLE_NAME
        // );
        // let txs = sqlx::query_as::<_, SolanaTransaction>(&query)
        //     .fetch_all(&self.pool)
        //     .await?;

        // Ok(txs)
        Err(anyhow::anyhow!("Not implemented"))
    }
}
