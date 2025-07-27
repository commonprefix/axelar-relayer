use anyhow::Result;
pub mod task_retries;
pub mod gmp_events;
pub mod gmp_tasks;

// E - entity, P - primary key
#[cfg_attr(test, mockall::automock)]
pub trait Model<E, P> {
    fn upsert(&self, entity: E) -> impl std::future::Future<Output = Result<()>> + Send;
    fn find(&self, id: P) -> impl std::future::Future<Output = Result<Option<E>>> + Send;
    fn delete(&self, entity: E) -> impl std::future::Future<Output = Result<()>> + Send;
}

pub struct Models {}
