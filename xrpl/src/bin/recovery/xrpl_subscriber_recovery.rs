use dotenv::dotenv;

use relayer_base::config::config_from_yaml;
use relayer_base::redis::connection_manager;
use relayer_base::{
    database::PostgresDB,
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use xrpl::{client::XRPLClient, config::XRPLConfig, subscriber::XrplSubscriber};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let events_queue = Queue::new(&config.common_config.queue_address, "events", 1).await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create PostgresDB: {}", e))?;

    let xrpl_client = XRPLClient::new(&config.xrpl_rpc, 3)?;
    let xrpl_subscriber =
        XrplSubscriber::new(xrpl_client, postgres_db, "recovery".to_string()).await?;
    let mut subscriber = Subscriber::new(xrpl_subscriber);
    let txs: Vec<&str> = vec![
        "80B60B79FF9402BDFAD32AD531E7679206E67C29B8F048162F0FFBEF59814D32",
        "DA8337CA5B9506C28929642E126D1B5D80D736C2C4D9ACEE255881ABD7B4AD9B",
        "d2ab1e62b3a21b91fd96430ac96bc1546096b824055cfa2933f6dce454ba6601",
        "A8D484C434D5EC7DC829775FEA96B713A4F807BF7412343562B51598CB40DB4F",
        "5E4C0FBFBAD8D1E15CBA1598245B4EF25E531C2C74AF26A47CC373E8A1096D47",
        "3FAC94FFD3D43094216A5C20FF54AF39D7C01D1E2381C54ACB0045F6CF9EF35C",
        "F124E4FCD0ED2514A8655FCA904073590469880E0EF42C44EADEEB0814112673",
        "44F32362AE12350C2E6BF27EE251A12DAB5094F477D117C4790ADE0AD0F1966D",
        "8AB417FD03549A5FBF9880CDFEF3A99EB00ACB6EDA62B6CD9ADAECDB9E9C0F3D",
        "3BD8F2DC26A8DAB6219BD453369E7FF932E224B4405253B2FA2AF606C07C2BDE",
        "23127CDC0B46863C453D69517C11F514EC293A17DC87CC2A3D7CEEE15B8BA1E2",
        "751FE03A35711903B27C8D65C29F5E52E2993E338796A979E2960868A698A737",
    ];

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    setup_heartbeat("heartbeat:subscriber_recovery".to_owned(), redis_conn, None);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.recover_txs(
            txs.into_iter().map(|s| s.to_string()).collect(),
            Arc::clone(&events_queue),
        ) => {},

    }

    events_queue.close().await;

    Ok(())
}
