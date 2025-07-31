use crate::config::Config;
use crate::logging::setup_logging;
use redis::Commands;
use tracing::{debug, error};

pub async fn heartbeats_loop(common_config: &Config) -> ! {
    let _guard = setup_logging(common_config);
    let redis_client = redis::Client::open(common_config.redis_server.clone())
        .expect("Failed to connect to redis server");
    let redis_pool = r2d2::Pool::builder()
        .build(redis_client)
        .expect("Failed to create redis pool");
    let client = reqwest::Client::new();
    loop {
        debug!("Sending heartbeats to sentry monitoring endpoint");

        for (key, url) in common_config.heartbeats.iter() {
            let redis_key = format!("heartbeat:{}", key);
            let mut redis_conn = redis_pool.get().expect("Failed to get redis connection");
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
