use async_trait::async_trait;
use coingecko::CoinGeckoClient;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{error, info, warn};

use crate::config::{Config, PriceFeedConfig};
use crate::database::Database;

#[async_trait]
trait PriceFeed {
    async fn fetch(&self, pairs: &[String]) -> FetchResult;
}

type FetchResult = Result<Vec<Option<Decimal>>, anyhow::Error>;

struct CoinGeckoPriceFeed {
    client: CoinGeckoClient,
    coin_ids: HashMap<String, String>,
    supported_vs_currencies: Vec<String>,
}

impl CoinGeckoPriceFeed {
    pub async fn new(config: &PriceFeedConfig) -> Result<Self, anyhow::Error> {
        let client = CoinGeckoClient::new_with_pro_api_key(
            config
                .auth
                .get("coingecko")
                .ok_or(anyhow::anyhow!("coingecko auth not found"))?
                .as_str(),
        );
        let supported_vs_currencies = client.supported_vs_currencies().await?;

        Ok(Self {
            client,
            coin_ids: config
                .coin_ids
                .get("coingecko")
                .expect("coingecko coin ids not found")
                .clone(),
            supported_vs_currencies,
        })
    }

    fn get_coin_id(&self, symbol: &str) -> Option<String> {
        self.coin_ids.get(symbol).map(|id| id.to_string())
    }
}

#[async_trait]
impl PriceFeed for CoinGeckoPriceFeed {
    async fn fetch(&self, pairs: &[String]) -> FetchResult {
        let (coin_ids, vs_currencies): (Vec<_>, Vec<_>) = pairs
            .iter()
            .filter_map(|pair| {
                let Some((token_a, token_b)) = pair.split_once('/') else {
                    warn!("Bad pair format: {pair}");
                    return None;
                };

                let vs = token_b.to_lowercase();
                if !self.supported_vs_currencies.contains(&vs) {
                    warn!("{vs} is not supported as a vs_currency");
                    return None;
                }

                match self.get_coin_id(token_a) {
                    Some(coin_id) => Some((coin_id, vs)),
                    None => {
                        warn!("Couldn't find coin_id for {token_a}");
                        None
                    }
                }
            })
            .unzip();

        let quotes = self
            .client
            .price(&coin_ids, &vs_currencies, false, false, false, true)
            .await?;

        Ok(pairs
            .iter()
            .map(|pair| {
                let (token_a, token_b) = pair.split_once('/')?;
                self.get_coin_id(token_a).and_then(|coin_id| {
                    quotes
                        .get(&coin_id)
                        .and_then(|p| serde_json::to_value(p).ok()) // `price` returns a struct that needs a detour through `serde_json::Value`
                        .and_then(|v| v.get(token_b.to_lowercase()).cloned())
                        .and_then(|v| v.as_f64().and_then(Decimal::from_f64))
                })
            })
            .collect())
    }
}

pub struct PriceFeeder<DB: Database> {
    feeds: Vec<Box<dyn PriceFeed>>,
    pairs: Vec<String>,
    db: DB,
}

impl<DB: Database> PriceFeeder<DB> {
    pub async fn new(config: &Config, db: DB) -> Result<Self, anyhow::Error> {
        let price_feed = CoinGeckoPriceFeed::new(&config.price_feed).await?;
        Ok(Self {
            feeds: vec![Box::new(price_feed)],
            pairs: config.price_feed.pairs.clone(),
            db,
        })
    }

    async fn store_prices(&self, prices: Vec<Option<Decimal>>) -> Result<(), anyhow::Error> {
        for (pair, price) in self.pairs.iter().zip(prices.iter()) {
            if let Some(price) = price {
                let _: () = self.db.store_price(pair, *price).await?;
            } else {
                warn!("No price returned for {}", pair);
            }
        }

        Ok(())
    }
    pub async fn run(&self) -> Result<(), anyhow::Error> {
        loop {
            // TODO: When there are more feeds, aggregate the results
            for feed in &self.feeds {
                if let Err(e) = async {
                    let prices = feed.fetch(&self.pairs).await?;
                    self.store_prices(prices).await?;
                    Ok::<_, anyhow::Error>(())
                }
                .await
                {
                    error!("Failed to update prices: {:?}", e);
                } else {
                    info!("Prices updated successfully");
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }
}
