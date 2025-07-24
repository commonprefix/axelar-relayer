use sqlx::{PgPool, Row};
use std::future::Future;

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

pub trait TONWalletQueryId {
    fn get_query_id(&self, address: &str) -> impl Future<Output = anyhow::Result<(i32, i32)>>;
    fn update_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
    ) -> impl Future<Output = anyhow::Result<()>>;
    fn upsert_query_id(
        &self,
        address: &str,
        shift: i32,
        bitnumber: i32,
        timeout: i32,
    ) -> impl Future<Output = anyhow::Result<()>>;
}

impl TONWalletQueryId for PgTONWalletQueryIdModel {
    async fn get_query_id(&self, address: &str) -> anyhow::Result<(i32, i32)> {
        let query = format!("SELECT shift, bitnumber FROM {} WHERE address = $1 AND expires_at >= CURRENT_TIMESTAMP", PG_TABLE_NAME);
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
        let query = format!("UPDATE {} SET shift = $1, bitnumber = $2, updated_at = CURRENT_TIMESTAMP WHERE address = $3", PG_TABLE_NAME);
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
            INSERT INTO {} (address, shift, bitnumber, expires_at)
            VALUES ($1, $2, $3, CURRENT_TIMESTAMP + ($4 * INTERVAL '1 second'))
            ON CONFLICT (address) DO UPDATE
            SET shift = EXCLUDED.shift,
                bitnumber = EXCLUDED.bitnumber,
                expires_at = EXCLUDED.expires_at",
            PG_TABLE_NAME
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
