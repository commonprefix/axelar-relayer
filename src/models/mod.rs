mod xrpl_transaction;

use anyhow::Result;
pub use xrpl_transaction::{
    PgXrplTransactionModel, XrplTransaction, XrplTransactionSource, XrplTransactionStatus,
    XrplTransactionType,
};

pub trait Model {
    type Entity;
    type PrimaryKey;

    async fn upsert(&self, entity: Self::Entity) -> Result<()>;
    async fn find(&self, id: Self::PrimaryKey) -> Result<Option<Self::Entity>>;
    async fn delete(&self, entity: Self::Entity) -> Result<()>;
}

pub struct Models {
    pub xrpl_transaction: PgXrplTransactionModel,
}
