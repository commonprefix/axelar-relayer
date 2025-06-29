use dotenv::dotenv;

use redis::Commands;
use relayer_base::{config::Config, utils::setup_logging};
use tracing::{debug, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network))?;

    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    let client = reqwest::Client::new();

    let _guard = setup_logging(&config);

    loop {
        debug!("Sending heartbeats to sentry monitoring endpoint");

        for (key, url) in config.heartbeats.iter() {
            let redis_key = format!("heartbeat:{}", key);
            let mut redis_conn = redis_pool.get().unwrap();
            if redis_conn.get(redis_key).unwrap_or(0) == 1 {
                match client.get(url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            debug!(
                                "Successfully sent heartbeat to sentry monitoring endpoint for {}",
                                key
                            );
                        } else {
                            error!(
                                "Failed to send heartbeat to sentry monitoring endpoint for {}: {:?}",
                                key,
                                response
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to send heartbeat to sentry monitoring endpoint for {}: {}",
                            key, e
                        );
                    }
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
