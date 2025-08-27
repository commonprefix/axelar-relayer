use anyhow::Result;
use futures::Future;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub trait SolanaSignatureModel {
    fn find(&self, id: String) -> impl Future<Output = Result<Option<SolanaSignature>>> + Send;
    fn upsert(&self, tx: SolanaSignature) -> impl Future<Output = Result<bool>> + Send;
    fn delete(&self, tx: SolanaSignature) -> impl Future<Output = Result<()>> + Send;
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SolanaSignature {
    pub signature: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const PG_TABLE_NAME: &str = "solana_signatures";
#[derive(Debug, Clone)]
pub struct PgSolanaSignatureModel {
    pool: PgPool,
}

impl SolanaSignatureModel for PgSolanaSignatureModel {
    async fn find(&self, id: String) -> Result<Option<SolanaSignature>> {
        let query = format!("SELECT * FROM {} WHERE signature = $1", PG_TABLE_NAME);
        let tx = sqlx::query_as::<_, SolanaSignature>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(tx)
    }

    async fn upsert(&self, tx: SolanaSignature) -> Result<bool> {
        let query = format!(
            "INSERT INTO {} \
             (signature, created_at) \
             VALUES ($1,NOW()) \
             ON CONFLICT (signature) DO NOTHING \
            ",
            PG_TABLE_NAME
        );

        let res = sqlx::query(&query)
            .bind(tx.signature)
            .execute(&self.pool)
            .await?;

        let inserted = res.rows_affected() > 0;

        Ok(inserted)
    }

    async fn delete(&self, tx: SolanaSignature) -> Result<()> {
        let query = format!("DELETE FROM {} WHERE signature = $1", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(tx.signature)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

impl PgSolanaSignatureModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
