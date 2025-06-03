use anyhow::Result;
pub mod task_retries;

pub trait Model {
    type Entity;
    type PrimaryKey;

    fn upsert(&self, entity: Self::Entity) -> impl std::future::Future<Output = Result<()>> + Send;
    fn find(
        &self,
        id: Self::PrimaryKey,
    ) -> impl std::future::Future<Output = Result<Option<Self::Entity>>> + Send;
    fn delete(&self, entity: Self::Entity) -> impl std::future::Future<Output = Result<()>> + Send;
}

pub struct Models {}
