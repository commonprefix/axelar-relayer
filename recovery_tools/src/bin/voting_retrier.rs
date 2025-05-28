use dotenv::dotenv;
use sqlx::PgPool;

use relayer_base::{config::Config, queue::Queue, utils::setup_logging};
use xrpl::models::xrpl_transaction::PgXrplTransactionModel;

use xrpl::voting_retrier::VotingRetrier;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network)).unwrap();

    let _guard = setup_logging(&config);

    let tasks_queue = Queue::new(&config.queue_address, "tasks").await;
    let events_queue = Queue::new(&config.queue_address, "events").await;
    let pg_pool = PgPool::connect(&config.postgres_url).await.unwrap();
    let redis_client = redis::Client::open(config.redis_server.clone()).unwrap();
    let redis_pool = r2d2::Pool::builder().build(redis_client).unwrap();
    let xrpl_transaction_model = PgXrplTransactionModel {
        pool: pg_pool.clone(),
    };
    let voting_retrier = VotingRetrier::new(
        events_queue,
        tasks_queue,
        xrpl_transaction_model,
        redis_pool,
    );

    voting_retrier.work().await;

    Ok(())
}
