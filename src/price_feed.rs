use std::{collections::HashMap, future::Future, pin::Pin};

use chrono::{DateTime, Utc};
use coingecko::CoinGeckoClient;
use r2d2::Pool;
use redis::Commands;
use tracing::{error, info, warn};

use crate::config::{Config, PriceFeedConfig};

trait PriceFeed: Send + Sync {
    fn fetch<'a>(
        &'a self,
        pairs: &'a [String],
    ) -> Pin<Box<dyn Future<Output = FetchResult> + Send + 'a>>;
}

type FetchResult = Result<Vec<Option<f64>>, anyhow::Error>;

struct CoinGeckoPriceFeed {
    client: CoinGeckoClient,
    coin_ids: HashMap<String, String>,
    supported_vs_currencies: Vec<String>,
}

const COINGECKO_API_PRO_URL: &str = "https://pro-api.coingecko.com/api/v3";

impl CoinGeckoPriceFeed {
    pub async fn new(config: &PriceFeedConfig) -> Result<Self, anyhow::Error> {
        let client = CoinGeckoClient::new_with_api_key(
            COINGECKO_API_PRO_URL,
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

impl PriceFeed for CoinGeckoPriceFeed {
    fn fetch<'a>(
        &'a self,
        pairs: &'a [String],
    ) -> Pin<Box<dyn Future<Output = FetchResult> + Send + 'a>> {
        Box::pin(async move {
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
                            .and_then(|p| {
                                // check that the price is less than 10 minutes old
                                let _ = p.last_updated_at
                                    .and_then(|last_updated| {
                                        DateTime::<Utc>::from_timestamp(last_updated as i64, 0)
                                    })
                                    .map(|last_updated| {
                                        if last_updated + chrono::Duration::minutes(10) < Utc::now()
                                        {
                                            warn!("Last updated at for {coin_id} is older than 10 minutes");
                                        }
                                    });

                                // `price` returns a struct that needs a detour through `serde_json::Value`
                                serde_json::to_value(p).ok()
                            })
                            .and_then(|v| v.get(token_b.to_lowercase()).cloned())
                            .and_then(|v| v.as_f64())
                    })
                })
                .collect())
        })
    }
}

pub struct PriceFeeder {
    feeds: Vec<Box<dyn PriceFeed>>,
    pairs: Vec<String>,
    redis_pool: Pool<redis::Client>,
}

impl PriceFeeder {
    pub async fn new(
        config: &Config,
        redis_pool: Pool<redis::Client>,
    ) -> Result<Self, anyhow::Error> {
        let price_feed = CoinGeckoPriceFeed::new(&config.price_feed).await?;
        Ok(Self {
            feeds: vec![Box::new(price_feed)],
            pairs: config.price_feed.pairs.clone(),
            redis_pool,
        })
    }

    async fn store_prices(&self, prices: Vec<Option<f64>>) -> Result<(), anyhow::Error> {
        let mut redis_client = self
            .redis_pool
            .get()
            .map_err(|e| anyhow::anyhow!("Failed to get Redis client: {}", e))?;

        for (pair, price) in self.pairs.iter().zip(prices.iter()) {
            if let Some(price) = price {
                let _: () = redis_client.set_ex(pair, price, 60 * 2).map_err(|e| {
                    anyhow::anyhow!("Failed to store price for {} in Redis: {}", pair, e)
                })?;
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
