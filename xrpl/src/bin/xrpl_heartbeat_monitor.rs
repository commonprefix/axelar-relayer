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

    let urls = vec![
        config.heartbeats.subscriber,
        config.heartbeats.distributor,
        config.heartbeats.includer,
        config.heartbeats.ingestor,
        config.heartbeats.ticket_creator,
        config.heartbeats.funder,
        config.heartbeats.price_feed,
        config.heartbeats.proof_retrier,
        config.heartbeats.ticket_monitor,
    ];

    loop {
        debug!("Sending heartbeats to sentry monitoring endpoint");

        for url in urls.iter() {
            let mut redis_conn = redis_pool.get().unwrap();
            if redis_conn.get(url).unwrap_or(0) == 1 {
                match client.get(url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            debug!(
                                "Successfully sent heartbeat to sentry monitoring endpoint for {}",
                                url
                            );
                        } else {
                            error!(
                                "Failed to send heartbeat to sentry monitoring endpoint for {}: {:?}",
                                url,
                                response
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to send heartbeat to sentry monitoring endpoint for {}: {}",
                            url, e
                        );
                    }
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
