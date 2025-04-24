use std::{future::Future, str::FromStr};

use redis::Commands;
use rust_decimal::Decimal;

pub trait PriceViewTrait {
    fn get_price(&self, pair: &str) -> impl Future<Output = Result<Decimal, anyhow::Error>>;
}

pub struct PriceView {
    pub redis_pool: r2d2::Pool<redis::Client>,
}

impl PriceView {
    pub fn new(redis_pool: r2d2::Pool<redis::Client>) -> Self {
        Self { redis_pool }
    }
}

#[cfg_attr(test, mockall::automock)]
impl PriceViewTrait for PriceView {
    async fn get_price(&self, pair: &str) -> Result<Decimal, anyhow::Error> {
        let (symbol_a, symbol_b) = pair
            .split_once('/')
            .ok_or(anyhow::anyhow!("Invalid pair"))?;

        if symbol_a == symbol_b {
            return Ok(Decimal::ONE);
        }

        let mut conn = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get redis connection: {}", e))?;
        let price: String = conn.get(pair)?;

        Ok(Decimal::from_str(&price)?)
    }
}
