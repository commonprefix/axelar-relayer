use crate::config::Config;
use crate::redis::connection_manager;
use crate::utils::setup_logging;
use redis::AsyncTypedCommands;
use tracing::{debug, error};

pub async fn heartbeats_loop(common_config: &Config) -> ! {
    let _guard = setup_logging(common_config);

    let redis_client = redis::Client::open(common_config.redis_server.clone())
        .expect("Failed to connect to redis server");
    let mut redis_conn = connection_manager(redis_client, None, None, None)
        .await
        .expect("Failed to connect to redis server");

    let client = reqwest::Client::new();
    loop {
        debug!("Sending heartbeats to sentry monitoring endpoint");

        for (key, url) in common_config.heartbeats.iter() {
            let redis_key = format!("heartbeat:{}", key);
            let res: Option<String> = redis_conn.get(redis_key.as_str()).await.unwrap_or(None);
            let value: u8 = res.and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);

            if value == 1 {
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
