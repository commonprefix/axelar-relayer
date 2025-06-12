use anyhow::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Type};
use xrpl_amplifier_types::msg::XRPLMessageType;
use xrpl_api::Transaction;

use relayer_base::{models::Model, utils::extract_and_decode_memo};

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
#[sqlx(type_name = "tx_type_enum", rename_all = "PascalCase")]
pub enum XrplTransactionType {
    TicketCreate,
    Payment,
    SignerListSet,
    TrustSet,
    Unexpected,
}

impl Default for XrplTransactionType {
    fn default() -> Self {
        Self::Unexpected
    }
}

impl TryFrom<Transaction> for XrplTransactionType {
    type Error = anyhow::Error;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        match tx {
            Transaction::Payment(_) => Ok(XrplTransactionType::Payment),
            Transaction::TicketCreate(_) => Ok(XrplTransactionType::TicketCreate),
            Transaction::SignerListSet(_) => Ok(XrplTransactionType::SignerListSet),
            Transaction::TrustSet(_) => Ok(XrplTransactionType::TrustSet),
            _ => Err(anyhow::anyhow!("Unknown transaction type: {:?}", tx)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
#[sqlx(type_name = "status_enum", rename_all = "PascalCase")]
pub enum XrplTransactionStatus {
    Detected,
    Initialized,
    Verified,
    Routed,
}

#[derive(Debug, Serialize, Deserialize, Clone, Type)]
#[sqlx(type_name = "source_enum", rename_all = "PascalCase")]
pub enum XrplTransactionSource {
    Prover,
    User,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct XrplTransaction {
    pub tx_hash: String,
    pub tx: String,
    pub tx_type: XrplTransactionType,
    pub message_id: Option<String>,
    pub message_type: String,
    pub status: XrplTransactionStatus,
    pub verify_task: Option<String>,
    pub verify_tx: Option<String>,
    pub quorum_reached_task: Option<String>,
    pub route_tx: Option<String>,
    pub source: XrplTransactionSource,
    pub sequence: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl XrplTransaction {
    pub fn from_native_transaction(
        tx: &Transaction,
        multisig_address: &str,
    ) -> Result<XrplTransaction, anyhow::Error> {
        let common = tx.common();
        let tx_hash = common
            .hash
            .clone()
            .ok_or(anyhow::anyhow!("No hash found"))?
            .to_lowercase();

        // Calculate created_at from ledger timestamp + ripple epoch
        let ledger_timestamp = Decimal::from(common.date.ok_or(anyhow::anyhow!("No date found"))?);
        let ripple_epoch = Decimal::from(946684800);
        let created_at = chrono::DateTime::from_timestamp(
            (ledger_timestamp + ripple_epoch)
                .to_string()
                .parse::<i64>()?,
            0,
        )
        .ok_or(anyhow::anyhow!(
            "Failed to convert ledger timestamp to chrono datetime"
        ))?;

        // Get message type from memos
        let memos = common.memos.clone();
        let message_type_str =
            extract_and_decode_memo(&memos, "type").map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let message_type: XRPLMessageType =
            serde_json::from_str(&format!("\"{}\"", message_type_str)).map_err(|e| {
                anyhow::anyhow!(format!(
                    "Failed to parse message type {}: {}",
                    message_type_str, e
                ))
            })?;

        Ok(XrplTransaction {
            tx_hash: tx_hash.clone(),
            tx: serde_json::to_string(&tx).unwrap_or_default(),
            tx_type: XrplTransactionType::try_from(tx.clone()).unwrap_or_default(),
            message_id: Some(tx_hash),
            message_type: message_type.to_string(),
            status: XrplTransactionStatus::Detected,
            verify_task: None,
            verify_tx: None,
            quorum_reached_task: None,
            route_tx: None,
            source: if tx.common().account == multisig_address {
                XrplTransactionSource::Prover
            } else {
                XrplTransactionSource::User
            },
            sequence: Some(tx.common().ticket_sequence.unwrap_or(tx.common().sequence) as i64),
            created_at,
        })
    }
}

const PG_TABLE_NAME: &str = "xrpl_transactions";
#[derive(Debug, Clone)]
pub struct PgXrplTransactionModel {
    pub pool: PgPool,
}

impl Model for PgXrplTransactionModel {
    type Entity = XrplTransaction;
    type PrimaryKey = String;

    async fn find(&self, id: Self::PrimaryKey) -> Result<Option<Self::Entity>> {
        let query = format!("SELECT * FROM {} WHERE tx_hash = $1", PG_TABLE_NAME);
        let tx = sqlx::query_as::<_, XrplTransaction>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(tx)
    }

    async fn upsert(&self, tx: XrplTransaction) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (tx_hash, tx, tx_type, message_id, message_type, status, verify_task, verify_tx, quorum_reached_task, route_tx, source, sequence, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) ON CONFLICT (tx_hash) DO UPDATE SET tx = $2, tx_type = $3, message_id = $4, message_type = $5, status = $6, verify_task = $7, verify_tx = $8, quorum_reached_task = $9, route_tx = $10, source = $11, sequence = $12, created_at = $13 RETURNING *",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(tx.tx_hash)
            .bind(tx.tx)
            .bind(tx.tx_type)
            .bind(tx.message_id)
            .bind(tx.message_type)
            .bind(tx.status)
            .bind(tx.verify_task)
            .bind(tx.verify_tx)
            .bind(tx.quorum_reached_task)
            .bind(tx.route_tx)
            .bind(tx.source)
            .bind(tx.sequence)
            .bind(tx.created_at)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn delete(&self, tx: XrplTransaction) -> Result<()> {
        let query = format!("DELETE FROM {} WHERE tx_hash = $1", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx.tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl PgXrplTransactionModel {
    pub async fn update_status(&self, tx_hash: &str, status: XrplTransactionStatus) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = $1 WHERE tx_hash = $2",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(status)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_verify_task(&self, tx_hash: &str, verify_task: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET verify_task = $1 WHERE tx_hash = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(verify_task)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_verify_tx(&self, tx_hash: &str, verify_tx: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET verify_tx = $1 WHERE tx_hash = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(verify_tx)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_quorum_reached_task(
        &self,
        tx_hash: &str,
        quorum_reached_task: &str,
    ) -> Result<()> {
        let query = format!(
            "UPDATE {} SET quorum_reached_task = $1 WHERE tx_hash = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(quorum_reached_task)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_route_tx(&self, tx_hash: &str, route_tx: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET route_tx = $1 WHERE tx_hash = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(route_tx)
            .bind(tx_hash)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn find_expired_events(&self) -> Result<Vec<XrplTransaction>> {
        let query = format!(
            "SELECT * FROM {} WHERE quorum_reached_task IS NULL AND verify_tx IS NOT NULL AND created_at < NOW() - INTERVAL '5 minutes'",
            PG_TABLE_NAME
        );
        let txs = sqlx::query_as::<_, XrplTransaction>(&query)
            .fetch_all(&self.pool)
            .await?;

        Ok(txs)
    }
}