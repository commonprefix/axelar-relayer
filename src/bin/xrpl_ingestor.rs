use dotenv::dotenv;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};

use axelar_relayer::{
    config::Config,
    gmp_api,
    ingestor::Ingestor,
    payload_cache::PayloadCache,
    price_view::PriceView,
    queue::Queue,
    utils::{setup_heartbeat, setup_logging},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);
    setup_heartbeat(config.heartbeats.ingestor.clone());

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let events_queue = Queue::new(&config.queue_address, "events").await;
    let gmp_api = Arc::new(gmp_api::GmpApi::new(&config, true).unwrap());
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let price_view = PriceView::new(redis_pool.clone());
    let payload_cache = PayloadCache::new(redis_pool);
    let ingestor = Ingestor::new(gmp_api.clone(), config.clone(), price_view, payload_cache);

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
        _ = ingestor.run(events_queue.clone(), tasks_queue.clone()) => {},
    }

    tasks_queue.close().await;
    events_queue.close().await;

    Ok(())
}
