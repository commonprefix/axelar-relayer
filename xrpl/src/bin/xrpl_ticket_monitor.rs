use dotenv::dotenv;

use relayer_base::{config::Config, utils::setup_logging};
use tracing::{debug, error};
use xrpl::client::XRPLClient;
use xrpl_api::Ticket;
use xrpl_types::AccountId;

const RETRIES: u8 = 4;
const THRESHOLD: u8 = 150;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let network = std::env::var("NETWORK").expect("NETWORK must be set");
    let config = Config::from_yaml(&format!("config.{}.yaml", network))?;

    let _guard = setup_logging(&config);

    let client = XRPLClient::new(&config.xrpl_rpc, RETRIES as usize)?;
    let account = AccountId::from_address(&config.xrpl_multisig).unwrap();

    loop {
        debug!("Checking tickets for account: {}", account.to_address());

        let maybe_tickets: Result<Vec<Ticket>, anyhow::Error> =
            client.get_available_tickets_for_account(&account).await;

        match maybe_tickets {
            Ok(tickets) => {
                let ticket_count = tickets.len() as u8;
                debug!("Ticket count: {}", ticket_count);
                debug!("Tickets: {:?}", tickets);
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
