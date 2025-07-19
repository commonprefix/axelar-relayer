use dotenv::dotenv;
use sqlx::PgPool;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::queue::Queue;
use relayer_base::subscriber::Subscriber;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;
use ton::config::TONConfig;
use ton::subscriber::TONSubscriber;
use tonlib_core::TonAddress;
use relayer_base::error::SubscriberError;
use ton::client::TONRpcClient;
use ton::ton_trace::PgTONTraceModel;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config.common_config);

    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let ton_gateway = config.ton_gateway;
    let ton_gas_service = config.ton_gas_service;
    let gateway_account = TonAddress::from_base64_url(ton_gateway.as_str())?;
    let gas_service_account = TonAddress::from_base64_url(ton_gas_service.as_str())?;
    
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:subscriber".to_owned(), redis_pool);
    
    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;

    let ton_traces = PgTONTraceModel::new(pg_pool.clone());
    
    let mut handles: Vec<JoinHandle<()>> = vec![];
    
    let client = TONRpcClient::new(config.ton_rpc.clone(), config.ton_api_key.clone(), 5, 5, 30)
        .await
        .map_err(|e| error_stack::report!(SubscriberError::GenericError(e.to_string())))
        .unwrap();

    for acct in [gateway_account, gas_service_account] {
        let ton_sub = TONSubscriber::new(
            client.clone(),
            postgres_db.clone(),
            acct.to_string(),
            config.common_config.chain_name.clone(),
            ton_traces.clone()
        )
            .await?;
        let mut sub = Subscriber::new(ton_sub);
        let queue_clone = events_queue.clone();
        let handle = tokio::spawn(async move {
            sub.run(acct, queue_clone).await;
        });
        handles.push(handle);
    }

    tokio::select! {
        _ = sigint.recv()  => {},
        _ = sigterm.recv() => {},
    }

    for handle in handles {
        handle.abort();
    }

    events_queue.close().await;

    Ok(())
}
