use crate::database::Database;
use crate::utils::ThreadSafe;
use async_trait::async_trait;
use rust_decimal::Decimal;

#[async_trait::async_trait]
pub trait PriceViewTrait {
    async fn get_price(&self, pair: &str) -> Result<Decimal, anyhow::Error>;
}

#[derive(Clone)]
pub struct PriceView<DB: Database + ThreadSafe> {
    pub db: DB,
}

impl<DB> PriceView<DB>
where
    DB: Database + ThreadSafe,
{
    pub fn new(db: DB) -> Self {
        Self { db }
    }
}

#[cfg_attr(any(test, feature = "mocks"), mockall::automock)]
#[async_trait]
impl<DB: Database + ThreadSafe> PriceViewTrait for PriceView<DB> {
    async fn get_price(&self, pair: &str) -> Result<Decimal, anyhow::Error> {
        let (symbol_a, symbol_b) = pair
            .split_once('/')
            .ok_or(anyhow::anyhow!("Invalid pair"))?;

        if symbol_a == symbol_b {
            return Ok(Decimal::ONE);
        }

        let maybe_price = self
            .db
            .get_price(pair)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get price: {}", e))?;

        maybe_price.ok_or_else(|| anyhow::anyhow!("Price not found"))
    }
}
