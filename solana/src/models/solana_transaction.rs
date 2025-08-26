use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub trait SolanaTransactionModel {
    async fn find(&self, id: String) -> Result<Option<SolanaTransaction>>;
    async fn upsert(&self, tx: SolanaTransaction) -> Result<()>;
    async fn delete(&self, tx: SolanaTransaction) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SolanaTransaction {
    pub signature: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const PG_TABLE_NAME: &str = "solana_transactions";
#[derive(Debug, Clone)]
pub struct PgSolanaTransactionModel {
    pool: PgPool,
}

impl SolanaTransactionModel for PgSolanaTransactionModel {
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
             (signature, created_at) \
             VALUES ($1,NOW()) \
             ON CONFLICT (signature) DO UPDATE SET \
               created_at = NOW() \
            ",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(tx.signature)
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
}
