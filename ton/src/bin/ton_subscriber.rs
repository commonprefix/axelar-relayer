use dotenv::dotenv;
use tokio::signal::unix::{signal, SignalKind};
use relayer_base::config::Config;
use relayer_base::database::PostgresDB;
use relayer_base::queue::Queue;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use ton::subscriber::TONSubscriber;
use tonlib_core::TonAddress;
use relayer_base::subscriber::Subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config);

    let events_queue = Queue::new(&config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.postgres_url).await?;

    let ton_gateway = config.ton_gateway;
    let account = TonAddress::from_base64_url(ton_gateway.as_str())?;

    let ton_subscriber = TONSubscriber::new(
        config.ton_rpc,
        config.ton_api_key,
        postgres_db,
        "default".to_string(),
        config.chain_name,
    )
    .await?;

    let mut subscriber = Subscriber::new(ton_subscriber);
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:subscriber".to_owned(), redis_pool);

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = subscriber.run(account, events_queue.clone()) => {},
    }

    events_queue.close().await;

    Ok(())
}
