use super::Model;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct TaskRetries {
    pub message_id: String,
    pub retries: i64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

const PG_TABLE_NAME: &str = "task_retries";
#[derive(Debug, Clone)]
pub struct PgTaskRetriesModel {
    pub pool: PgPool,
}

impl Model<TaskRetries, String> for PgTaskRetriesModel {
    async fn find(&self, id: String) -> Result<Option<TaskRetries>> {
        let query = format!("SELECT * FROM {} WHERE message_id = $1", PG_TABLE_NAME);
        let entry = sqlx::query_as::<_, TaskRetries>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(entry)
    }

    async fn upsert(&self, entry: TaskRetries) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (message_id, retries, updated_at) VALUES ($1, $2, NOW()) ON CONFLICT (message_id) DO UPDATE SET retries = $2, updated_at = NOW() RETURNING *",
            PG_TABLE_NAME
        );

        sqlx::query(&query)
            .bind(entry.message_id)
            .bind(entry.retries)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn delete(&self, entry: TaskRetries) -> Result<()> {
        let query = format!("DELETE FROM {} WHERE message_id = $1", PG_TABLE_NAME);
        sqlx::query(&query)
            .bind(entry.message_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
