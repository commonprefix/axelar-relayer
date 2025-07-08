use anyhow::Result;
use async_trait::async_trait;
pub mod task_retries;

// E - entity, P - primary key
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait Model<E, P> {
    async fn upsert(&self, entity: E) -> Result<()>;
    async fn find(&self, id: P) -> Result<Option<E>>;
    async fn delete(&self, entity: E) -> Result<()>;
}

pub struct Models {}
