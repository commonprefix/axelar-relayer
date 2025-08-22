use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::price_view::PriceView;
use relayer_base::queue::Queue;
use relayer_base::redis::connection_manager;
use relayer_base::utils::setup_logging;
use relayer_base::{gmp_api, ingestor};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use ton::config::TONConfig;
use ton::gas_calculator::GasCalculator;
use ton::ingestor::TONIngestor;
use ton::parser::TraceParser;
use ton::ton_trace::PgTONTraceModel;
use tonlib_core::TonAddress;
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

    ingestor::run_ingestor(
        &tasks_queue,
        &events_queue,
        gmp_api,
        redis_conn,
        Arc::new(ton_ingestor),
    )
    .await?;

    Ok(())
}
