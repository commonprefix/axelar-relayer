use sqlx::{PgPool, Row};
use async_trait::async_trait;

const PG_TABLE_NAME: &str = "ton_wallet_query_id";

#[derive(Debug, Clone)]
pub struct PgTONWalletQueryIdModel {
    pool: PgPool,
}

impl PgTONWalletQueryIdModel {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
pub trait TONWalletQueryId {
    async fn get_query_id(&self, address: &str) -> anyhow::Result<(i32, i32)>;
    async fn update_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
    ) -> anyhow::Result<()>;
    async fn upsert_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
        timeout: i32,
    ) -> anyhow::Result<()>;
}

#[async_trait]
impl TONWalletQueryId for PgTONWalletQueryIdModel {
    async fn get_query_id(&self, address: &str) -> anyhow::Result<(i32, i32)> {
        let query = format!("SELECT shift, bitnumber FROM {PG_TABLE_NAME} WHERE address = $1 AND expires_at >= CURRENT_TIMESTAMP");
        let maybe_row = sqlx::query(&query)
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = maybe_row {
            let shift: i32 = row.get("shift");
            let bitnumber: i32 = row.get("bitnumber");
            Ok((shift, bitnumber))
        } else {
            Ok((-1, -1))
        }
    }

    async fn update_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
    ) -> anyhow::Result<()> {
        let query = format!("UPDATE {PG_TABLE_NAME} SET shift = $1, bitnumber = $2, updated_at = CURRENT_TIMESTAMP WHERE address = $3");
        sqlx::query(&query)
            .bind(shift)
            .bind(bitnumber)
            .bind(address)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn upsert_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
        timeout: i32,
    ) -> anyhow::Result<()> {
        let query = format!(
            "
            INSERT INTO {PG_TABLE_NAME} (address, shift, bitnumber, expires_at)
            VALUES ($1, $2, $3, CURRENT_TIMESTAMP + ($4 * INTERVAL '1 second'))
            ON CONFLICT (address) DO UPDATE
            SET shift = EXCLUDED.shift,
                bitnumber = EXCLUDED.bitnumber,
                expires_at = EXCLUDED.expires_at"
        );

        sqlx::query(&query)
            .bind(address)
            .bind(shift)
            .bind(bitnumber)
            .bind(timeout)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
