use crate::database::Database;
use rust_decimal::Decimal;
use std::future::Future;

pub trait PriceViewTrait {
    fn get_price(&self, pair: &str) -> impl Future<Output = Result<Decimal, anyhow::Error>>;
}

pub struct PriceView<DB: Database> {
    pub db: DB,
}

impl<DB: Database> PriceView<DB> {
    pub fn new(db: DB) -> Self {
        Self { db }
    }
}

#[cfg_attr(any(test, feature = "mocks"), mockall::automock)]
impl<DB: Database> PriceViewTrait for PriceView<DB> {
    #[tracing::instrument(skip(self))]
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
