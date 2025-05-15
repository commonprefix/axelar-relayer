use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, PgPool, Row, Type};
use xrpl_api::Transaction;

use super::Model;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct XrplTransaction {
    pub tx_hash: String,
    pub tx_type: XrplTransactionType,
    pub message_id: Option<String>,
    pub status: XrplTransactionStatus,
    pub verify_task: Option<String>,
    pub verify_tx: Option<String>,
    pub quorum_reached_task: Option<String>,
    pub route_tx: Option<String>,
    pub source: XrplTransactionSource,
    pub sequence: Option<i64>,
}

impl XrplTransaction {
    pub fn from_native_transaction(
        tx: &Transaction,
        multisig_address: &str,
    ) -> Result<XrplTransaction, anyhow::Error> {
        let tx_hash = tx
            .common()
            .hash
            .clone()
            .ok_or(anyhow::anyhow!("No hash found"))?
            .to_lowercase();
        Ok(XrplTransaction {
            tx_hash: tx_hash.clone(),
            tx_type: XrplTransactionType::try_from(tx.clone()).unwrap_or_default(),
            message_id: Some(tx_hash),
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
        })
    }
}

impl sqlx::FromRow<'_, PgRow> for XrplTransaction {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(XrplTransaction {
            tx_hash: row.get("tx_hash"),
            tx_type: row.get("tx_type"),
            message_id: row.get("message_id"),
            status: row.get("status"),
            verify_task: row.get("verify_task"),
            verify_tx: row.get("verify_tx"),
            quorum_reached_task: row.get("quorum_reached_task"),
            route_tx: row.get("route_tx"),
            source: row.get("source"),
            sequence: row.get("sequence"),
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
            "INSERT INTO {} (tx_hash, tx_type, message_id, status, verify_task, verify_tx, quorum_reached_task, route_tx, source, sequence) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT (tx_hash) DO UPDATE SET tx_type = $2, message_id = $3, status = $4, verify_task = $5, verify_tx = $6, quorum_reached_task = $7, route_tx = $8, source = $9, sequence = $10",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(tx.tx_hash)
            .bind(tx.tx_type)
            .bind(tx.message_id)
            .bind(tx.status)
            .bind(tx.verify_task)
            .bind(tx.verify_tx)
            .bind(tx.quorum_reached_task)
            .bind(tx.route_tx)
            .bind(tx.source)
            .bind(tx.sequence)
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
}
