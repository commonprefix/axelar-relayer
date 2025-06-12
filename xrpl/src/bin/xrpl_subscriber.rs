use dotenv::dotenv;

use relayer_base::{
    config::Config,
    database::PostgresDB,
    queue::Queue,
    subscriber::Subscriber,
    utils::{setup_heartbeat, setup_logging},
};
use tokio::signal::unix::{signal, SignalKind};
use xrpl_types::AccountId;

use xrpl::subscriber::XrplSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let events_queue = Queue::new(&config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();

    let account = AccountId::from_address(&config.xrpl_multisig).unwrap();

    let xrpl_subscriber =
        XrplSubscriber::new(&config.xrpl_rpc, postgres_db, "default".to_string()).await?;
    let mut subscriber = Subscriber::new(xrpl_subscriber);
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat(config.heartbeats.subscriber.clone(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.run(account.to_address(), events_queue.clone()) => {},
    }

    events_queue.close().await;

    Ok(())
}
