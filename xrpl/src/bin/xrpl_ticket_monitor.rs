use dotenv::dotenv;

use relayer_base::config::config_from_yaml;
use relayer_base::utils::setup_heartbeat;
use tracing::{debug, error};
use xrpl::client::{XRPLClient, XRPLClientTrait};
use xrpl::config::XRPLConfig;
use xrpl_api::Ticket;
use xrpl_types::AccountId;
use relayer_base::logging::setup_logging;

const RETRIES: u8 = 4;
const THRESHOLD: u8 = 150;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config: XRPLConfig = config_from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config.common_config);

    let client = XRPLClient::new(&config.xrpl_rpc, RETRIES as usize)?;
    let account = AccountId::from_address(&config.xrpl_multisig)?;

    let redis_client = redis::Client::open(config.common_config.redis_server.clone())?;
    let redis_pool = r2d2::Pool::builder().build(redis_client)?;

    setup_heartbeat("heartbeat:ticket_monitor".to_owned(), redis_pool);

    loop {
        debug!("Checking tickets for account: {}", account.to_address());

        let maybe_tickets: Result<Vec<Ticket>, anyhow::Error> =
            client.get_available_tickets_for_account(&account).await;

        match maybe_tickets {
            Ok(tickets) => {
                let ticket_count = tickets.len() as u8;
                debug!("Ticket count: {}", ticket_count);
                debug!(
                    "Tickets: {:?}",
                    tickets
                        .iter()
                        .map(|t| t.ticket_sequence)
                        .collect::<Vec<u32>>()
                );
                if ticket_count < THRESHOLD {
                    error!("Tickets are bellow threshold");
                    return Err(anyhow::anyhow!("Tickets are bellow threshold"));
                }
            }
            Err(e) => {
                error!("Error getting tickets: {}", e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
