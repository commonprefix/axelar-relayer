use dotenv::dotenv;
use relayer_base::config::config_from_yaml;
use relayer_base::database::PostgresDB;
use relayer_base::price_view::PriceView;
use relayer_base::queue::Queue;
use relayer_base::redis::connection_manager;
use relayer_base::utils::setup_logging;
use relayer_base::{gmp_api, ingestor};
use solana::config::SolanaConfig;
use solana::ingestor::SolanaIngestor;
use solana::parser::TransactionParser;
use solana::solana_transaction::PgSolanaTransactionModel;
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let tasks_queue = Queue::new(
        &config.common_config.queue_address,
        "ingestor_tasks",
        config.common_config.num_workers,
    )
    .await;
    let events_queue = Queue::new(
        &config.common_config.queue_address,
        "events",
        config.common_config.num_workers,
    )
    .await;
    let postgres_db = PostgresDB::new(&config.common_config.postgres_url).await?;

    let pg_pool = PgPool::connect(&config.common_config.postgres_url).await?;

    let gmp_api = gmp_api::construct_gmp_api(pg_pool.clone(), &config.common_config, true)?;
    let price_view = PriceView::new(postgres_db.clone());

    let mut our_addresses = vec![];
    for wallet in config.wallets {
        our_addresses.push(Pubkey::from_str(&wallet.public_key)?);
    }
    let gateway = Pubkey::from_str(&config.solana_gateway)?;
    let gas_service = Pubkey::from_str(&config.solana_gas_service)?;
    our_addresses.push(gateway);
    our_addresses.push(gas_service);

    let parser = TransactionParser::new(
        price_view,
        gateway,
        gas_service,
        config.common_config.chain_name,
    );

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_conn = connection_manager(redis_client, None, None, None).await?;

    let solana_transaction_model = PgSolanaTransactionModel::new(pg_pool.clone());
    let solana_ingestor = SolanaIngestor::new(parser, solana_transaction_model);

    ingestor::run_ingestor(
        &tasks_queue,
        &events_queue,
        gmp_api,
        redis_conn,
        Arc::new(solana_ingestor),
    )
    .await?;

    Ok(())
}
