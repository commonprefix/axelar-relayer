use dotenv::dotenv;

use axelar_relayer::{
    config::Config,
    database::PostgresDB,
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.subscriber.clone());

    let events_queue = Queue::new(&config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();

    let mut subscriber = Subscriber::new_xrpl(&config.xrpl_rpc, postgres_db).await;
    let txs: Vec<&str> = vec![];

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.recover_txs(
            txs.into_iter().map(|s| s.to_string()).collect(),
            events_queue.clone(),
        ) => {},

    }

    events_queue.close().await;

    Ok(())
}
