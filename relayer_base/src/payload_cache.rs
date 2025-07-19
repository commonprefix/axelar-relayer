use std::future::Future;
use router_api::CrossChainId;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::database::Database;
use crate::gmp_api::gmp_types::GatewayV2Message;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PayloadCacheValue {
    pub message: GatewayV2Message,
    pub payload: String,
}

pub struct PayloadCache<DB: Database> {
    db: DB,
}

#[cfg_attr(any(test, feature = "mocks"), mockall::automock)]
pub trait PayloadCacheTrait {
    fn get(&self, cc_id: CrossChainId) -> impl Future<Output = Result<Option<PayloadCacheValue>, anyhow::Error>>;
    fn store(&self, cc_id: CrossChainId, value: PayloadCacheValue) -> impl Future<Output = Result<(), anyhow::Error>>;
    fn clear(&self, cc_id: CrossChainId) -> impl Future<Output = Result<(), anyhow::Error>>;
}


impl<DB: Database> PayloadCache<DB> {
    pub fn new(db: DB) -> Self {
        Self { db }
    }
}

impl<DB: Database> PayloadCacheTrait for PayloadCache<DB> {

    async fn get(
        &self,
        cc_id: CrossChainId,
    ) -> Result<Option<PayloadCacheValue>, anyhow::Error> {
        let value = self
            .db
            .get_payload(cc_id.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get payload from database: {}", e))?;

        debug!("Got payload for {}: {:?}", cc_id, value);

        if let Some(value) = value {
            let payload_cache_value: PayloadCacheValue = serde_json::from_str(value.as_str())?;
            Ok(Some(payload_cache_value))
        } else {
            Ok(None)
        }
    }

    async fn store(
        &self,
        cc_id: CrossChainId,
        value: PayloadCacheValue,
    ) -> Result<(), anyhow::Error> {
        let value = serde_json::to_string(&value)?;
        debug!("Storing payload cache value for {}: {}", cc_id, value);
        self.db
            .store_payload(cc_id, value)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to store payload: {}", e))?;

        Ok(())
    }

    async fn clear(&self, cc_id: CrossChainId) -> Result<(), anyhow::Error> {
        debug!("Clearing payload cache for {}", cc_id);
        self.db
            .clear_payload(cc_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to clear payload: {}", e))?;

        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::MockDatabase;
    use crate::gmp_api::gmp_types::GatewayV2Message;
    use mockall::predicate::*;
    use tokio;

    #[tokio::test]
    async fn test_store() {
        let mut mock_db = MockDatabase::new();
        let cc_id = CrossChainId::new("foo", "bar").unwrap();
        let message = GatewayV2Message {
            message_id: "message".to_string(),
            source_chain: "source chain".to_string(),
            source_address: "source address".to_string(),
            destination_address: "destination address".to_string(),
            payload_hash: "payload hash".to_string(),
        };
        let value = PayloadCacheValue {
            message: message.clone(),
            payload: "test_payload".into(),
        };
        let serialized = serde_json::to_string(&value).unwrap();

        mock_db
            .expect_store_payload()
            .with(eq(cc_id.clone()), eq(serialized.to_string()))
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let cache = PayloadCache::new(mock_db);
        let result = cache.store(cc_id, value).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_existing_hit() {
        let mut mock_db = MockDatabase::new();
        let cc_id = CrossChainId::new("foo", "bar").unwrap();
        let message = GatewayV2Message {
            message_id: "message".to_string(),
            source_chain: "source chain".to_string(),
            source_address: "source address".to_string(),
            destination_address: "destination address".to_string(),
            payload_hash: "payload hash".to_string(),
        };
        let value = PayloadCacheValue {
            message: message.clone(),
            payload: "test_payload".into(),
        };
        let serialized = serde_json::to_string(&value).unwrap();
        mock_db
            .expect_get_payload()
            .with(eq(cc_id.clone()))
            .returning(move |_| {
                let result = Some(serialized.clone());
                Box::pin(async move { Ok(result) })
            });

        let cache = PayloadCache::new(mock_db);
        let result = cache.get(cc_id).await.unwrap();

        assert_eq!(result, Some(value));
    }

    #[tokio::test]
    async fn test_get_existing_miss() {
        let mut mock_db = MockDatabase::new();
        let cc_id = CrossChainId::new("foo", "bar").unwrap();

        mock_db
            .expect_get_payload()
            .with(eq(cc_id.clone()))
            .returning(move |_| {
                Box::pin(async move { Ok(None) })
            });

        let cache = PayloadCache::new(mock_db);
        let result = cache.get(cc_id).await.unwrap();

        assert_eq!(result, None);
    }
}