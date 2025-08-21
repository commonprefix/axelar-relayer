use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::gmp_api;
use relayer_base::ingestor::{Ingestor, IngestorTrait};
use relayer_base::ingestor_worker::IngestorWorker;
use relayer_base::price_view::PriceView;
use relayer_base::queue::Queue;
use relayer_base::redis::connection_manager;
use relayer_base::utils::{setup_heartbeat, setup_logging};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use redis::aio::ConnectionManager;
use tokio::signal::unix::{signal, SignalKind};
use ton::config::TONConfig;
use ton::gas_calculator::GasCalculator;
use ton::ingestor::TONIngestor;
use ton::parser::TraceParser;
use ton::ton_trace::PgTONTraceModel;
use tonlib_core::TonAddress;
use tokio_util::sync::CancellationToken;
use tracing::info;
use relayer_base::gmp_api::{GmpApi, GmpApiDbAuditDecorator};
use relayer_base::models::gmp_events::PgGMPEvents;
use relayer_base::models::gmp_tasks::PgGMPTasks;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: TONConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(&config.common_config.queue_address, "ingestor_tasks").await;
    let events_queue = Queue::new(&config.common_config.queue_address, "events").await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;

    let gmp_api = gmp_api::construct_gmp_api(pg_pool.clone(), &config.common_config, true)?;
    let price_view = PriceView::new(postgres_db.clone());

    let mut our_addresses = vec![];
    for wallet in config.wallets {
        our_addresses.push(TonAddress::from_str(&wallet.address)?);
    }
    let gateway = TonAddress::from_str(&config.ton_gateway)?;
    let gas_service = TonAddress::from_str(&config.ton_gas_service)?;
    our_addresses.push(gateway.clone());
    our_addresses.push(gas_service.clone());

    let gas_calculator = GasCalculator::new(our_addresses);

    let parser = TraceParser::new(
        price_view,
        gateway,
        gas_service,
        gas_calculator,
        config.common_config.chain_name,
    );

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    let ton_trace_model = PgTONTraceModel::new(pg_pool.clone());
    let ton_ingestor = TONIngestor::new(parser, ton_trace_model);

    run_ingestor(&tasks_queue, &events_queue, gmp_api, redis_conn, ton_ingestor).await?;
    
    Ok(())
}

async fn run_ingestor(tasks_queue: &Arc<Queue>, events_queue: &Arc<Queue>, gmp_api: Arc<GmpApiDbAuditDecorator<GmpApi, PgGMPTasks, PgGMPEvents>>, redis_conn: ConnectionManager, chain_ingestor: Arc<dyn IngestorTrait>) -> Result<(), Error> {
    let worker = IngestorWorker::new(gmp_api, chain_ingestor.clone());
    let token = CancellationToken::new();
    let ingestor = Ingestor::new(worker, token.clone());
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    setup_heartbeat("heartbeat:price_feed".to_owned(), redis_conn, Some(token.clone()));
    let sigint_cloned_token = token.clone();
    let sigterm_cloned_token = token.clone();
    let ingestor_cloned_token = token.clone();
    let handle = tokio::spawn({
        let events = Arc::clone(&events_queue);
        let tasks = Arc::clone(&tasks_queue);
        async move {
            ingestor.run(events, tasks).await
        }
    });

    tokio::pin!(handle);

    tokio::select! {
        _ = sigint.recv()  => {
            sigint_cloned_token.cancel();
        },
        _ = sigterm.recv() => {
            sigterm_cloned_token.cancel();
        },
        _ = &mut handle => {
            info!("Ingestor stopped");
            ingestor_cloned_token.cancel();
        }
    }

    tasks_queue.close().await;
    events_queue.close().await;
    let _ = handle.await;
    Ok(())
}
