use dotenv::dotenv;

use tonlib_client::client::{TonClient, TonClientInterface, TonConnectionParams};
use tonlib_client::client::TonClientBuilder;
use tonlib_client::config::TESTNET_CONFIG;
use tonlib_core::TonAddress;
use relayer_base::config::Config;
use relayer_base::database::PostgresDB;
use relayer_base::queue::Queue;
use relayer_base::utils::setup_logging;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let events_queue = Queue::new(&config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();

    // let network = std::env::var("NETWORK").expect("NETWORK must be set");
    // let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();
    //
    // let _guard = setup_logging(&config);
    //
    // let events_queue = Queue::new(&config.queue_address, "events").await;
    // let postgres_db = PostgresDB::new(&config.postgres_url).await.unwrap();
    //
    // let gateway_address = &config.ton_gateway;
    //
    // let ton_subscriber = TONSubscriber::new(&config.ton_rpc, postgres_db).await?;
    // let mut subscriber = Subscriber::new(xrpl_subscriber);
    // let mut sigint = signal(SignalKind::interrupt())?;
    // let mut sigterm = signal(SignalKind::terminate())?;
    //
    // tokio::select! {
    //     _ = sigint.recv()  => {},
    //     _ = sigterm.recv() => {},
    //     _ = subscriber.run(account.to_address(), events_queue.clone()) => {},
    // }
    //
    // events_queue.close().await;

    Ok(())
}
