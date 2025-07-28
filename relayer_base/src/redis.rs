use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::Client;
use std::time::Duration;

pub async fn connection_manager(
    client: Client,
    conn_timeout: Option<Duration>,
    resp_timeout: Option<Duration>,
    retries: Option<usize>,
) -> redis::RedisResult<ConnectionManager> {
    let config = ConnectionManagerConfig::new()
        .set_connection_timeout(conn_timeout.unwrap_or_else(|| Duration::from_secs(2)))
        .set_response_timeout(resp_timeout.unwrap_or_else(|| Duration::from_secs(2)))
        .set_number_of_retries(retries.unwrap_or(2))
        .set_max_delay(10000); // 1000 = 1 second

    println!("{:?}", config);
    let conn = ConnectionManager::new_with_config(client, config).await?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use crate::redis::connection_manager;
    use redis::{AsyncTypedCommands, Client};
    use std::time::Duration;
    use testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::GenericImage;

    #[tokio::test]
    async fn test_multiplexed_connection() {
        let container = GenericImage::new("redis", "7.2.4")
            .with_exposed_port(9991.tcp())
            .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
            .start()
            .await
            .unwrap();

        let host = container.get_host().await.unwrap();
        let host_port = container.get_host_port_ipv4(6379).await.unwrap();

        let url = format!("redis://{host}:{host_port}");
        let client = Client::open(url.as_ref()).unwrap();
        let mut conn = connection_manager(client, Some(Duration::from_millis(100)), Some(Duration::from_millis(100)), Some(2)).await.unwrap();

        // Redis is running
        conn.set("foo", "bar").await.unwrap();
        let r = conn.get("foo").await.unwrap().unwrap();
        assert_eq!(r, "bar");

        // Stop redis, any writing should fail
        container.pause().await.unwrap();
        let r = conn.get("foo").await;
        assert!(r.is_err());

        // Restart redis, we should be able to continue on the same connection without intervention
        container.unpause().await.unwrap();
        let r = conn.get("foo").await.unwrap().unwrap();
        assert_eq!(r, "bar");
    }
}