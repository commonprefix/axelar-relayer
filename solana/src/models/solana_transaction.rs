use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_sdk::transaction::{Transaction, TransactionError};
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

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::Type)]
#[sqlx(type_name = "status_enum", rename_all = "PascalCase")]
pub enum SolanaStatus {
    Detected,
    Initialized,
    Verified,
    Routed,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SolanaTransaction {
    pub signature: String,
    pub signatures: Option<Vec<String>>,
    pub tx: String,
    pub status: SolanaStatus,
    pub source: SolanaTransactionSource,
    pub confirmation_status: Option<String>,
    pub verify_task: Option<String>,
    pub verify_tx: Option<String>,
    pub quorum_reached_task: Option<String>,
    pub route_tx: Option<String>,
    pub block_time: Option<chrono::DateTime<chrono::Utc>>,
    pub logs: Vec<String>,
    /// The accounts that were passed to an instruction.
    /// - first item: the program id
    /// - second item: the Pubkeys provided to the ix
    /// - third item: payload data
    pub ixs: sqlx::types::Json<Vec<IxRecord>>,
    pub inner_ixs: sqlx::types::Json<Vec<IxRecord>>,
    pub slot: i64,
    pub fee_lamports: i64,
    pub meta_err: Option<sqlx::types::Json<serde_json::Value>>,
    pub version: i16,
    pub loaded_addresses: Option<sqlx::types::Json<serde_json::Value>>,
    pub account_keys: Vec<String>,
    pub payer: Option<String>,
    pub pre_post_balances: Option<sqlx::types::Json<serde_json::Value>>,
    pub pre_post_token_balances: Option<sqlx::types::Json<serde_json::Value>>,
    pub compute_units: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IxRecord {
    pub program_id: String,    // base58
    pub accounts: Vec<String>, // base58
    pub data: String,          // base64 or hex
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
        let query = format!("SELECT * FROM {} WHERE signature = $1", PG_TABLE_NAME);
        let tx = sqlx::query_as::<_, SolanaTransaction>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(tx)
    }

    async fn upsert(&self, tx: SolanaTransaction) -> Result<()> {
        let query = format!(
            "INSERT INTO {} \
             (signature, signatures, tx, status, source, confirmation_status, verify_task, verify_tx, quorum_reached_task, route_tx, \
              block_time, logs, ixs, inner_ixs, slot, fee_lamports, meta_err, version, loaded_addresses, account_keys, payer, \
              pre_post_balances, pre_post_token_balances, compute_units, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,NOW()) \
             ON CONFLICT (signature) DO UPDATE SET \
               tx = EXCLUDED.tx, \
               status = EXCLUDED.status, \
               source = EXCLUDED.source, \
               confirmation_status = EXCLUDED.confirmation_status, \
               verify_task = EXCLUDED.verify_task, \
               verify_tx = EXCLUDED.verify_tx, \
               quorum_reached_task = EXCLUDED.quorum_reached_task, \
               route_tx = EXCLUDED.route_tx, \
               block_time = EXCLUDED.block_time, \
               logs = EXCLUDED.logs, \
               ixs = EXCLUDED.ixs, \
               inner_ixs = EXCLUDED.inner_ixs, \
               slot = EXCLUDED.slot, \
               fee_lamports = EXCLUDED.fee_lamports, \
               meta_err = EXCLUDED.meta_err, \
               version = EXCLUDED.version, \
               loaded_addresses = EXCLUDED.loaded_addresses, \
               account_keys = EXCLUDED.account_keys, \
               payer = EXCLUDED.payer, \
               pre_post_balances = EXCLUDED.pre_post_balances, \
               pre_post_token_balances = EXCLUDED.pre_post_token_balances, \
               compute_units = EXCLUDED.compute_units",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(tx.signature)
            .bind(tx.signatures)
            .bind(tx.tx)
            .bind(tx.status)
            .bind(tx.source)
            .bind(tx.confirmation_status)
            .bind(tx.verify_task)
            .bind(tx.verify_tx)
            .bind(tx.quorum_reached_task)
            .bind(tx.route_tx)
            .bind(tx.block_time)
            .bind(tx.logs)
            .bind(tx.ixs)
            .bind(tx.inner_ixs)
            .bind(tx.slot)
            .bind(tx.fee_lamports)
            .bind(tx.meta_err)
            .bind(tx.version)
            .bind(tx.loaded_addresses)
            .bind(tx.account_keys)
            .bind(tx.payer)
            .bind(tx.pre_post_balances)
            .bind(tx.pre_post_token_balances)
            .bind(tx.compute_units)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn delete(&self, tx: SolanaTransaction) -> Result<()> {
        let query = format!("DELETE FROM {} WHERE signature = $1", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx.signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl PgSolanaTransactionModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn update_status(&self, signature: &str, status: SolanaStatus) -> Result<()> {
        let query = format!(
            "UPDATE {} SET status = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(status)
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_verify_task(&self, signature: &str, verify_task: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET verify_task = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(verify_task)
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_verify_tx(&self, signature: &str, verify_tx: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET verify_tx = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(verify_tx)
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_quorum_reached_task(
        &self,
        signature: &str,
        quorum_reached_task: &str,
    ) -> Result<()> {
        let query = format!(
            "UPDATE {} SET quorum_reached_task = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(quorum_reached_task)
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_route_tx(&self, signature: &str, route_tx: &str) -> Result<()> {
        let query = format!(
            "UPDATE {} SET route_tx = $1 WHERE signature = $2",
            PG_TABLE_NAME
        );
        sqlx::query(&query)
            .bind(route_tx)
            .bind(signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn find_expired_events(&self) -> Result<Vec<SolanaTransaction>> {
        let query = format!(
            "SELECT * FROM {} WHERE quorum_reached_task IS NULL AND verify_tx IS NOT NULL AND created_at < NOW() - INTERVAL '5 minutes'",
            PG_TABLE_NAME
        );
        let txs = sqlx::query_as::<_, SolanaTransaction>(&query)
            .fetch_all(&self.pool)
            .await?;

        Ok(txs)
    }
}
