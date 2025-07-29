use std::sync::Arc;

use tracing::{debug, error, info};

use crate::config::XRPLConfig;
use relayer_base::gmp_api::{gmp_types::BroadcastRequest, GmpApiTrait};
use xrpl_multisig_prover;
use relayer_base::utils::ThreadSafe;

pub struct XrplTicketCreator<G: GmpApiTrait + ThreadSafe> {
    gmp_api: Arc<G>,
    config: XRPLConfig,
}

impl<G> XrplTicketCreator<G>
where G: GmpApiTrait + ThreadSafe
{
    pub fn new(gmp_api: Arc<G>, config: XRPLConfig) -> Self {
        Self { gmp_api, config }
    }

    async fn work(&self) {
        let ticket_create_msg =
            serde_json::to_value(xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate);
        if ticket_create_msg.is_err() {
            error!(
                "Failed to serialize TicketCreate message: {:?}",
                ticket_create_msg.err()
            );
            return;
        }

        let request = BroadcastRequest::Generic(ticket_create_msg.unwrap_or_default());

        let res = self
            .gmp_api
            .post_broadcast(
                self.config
                    .common_config
                    .axelar_contracts
                    .chain_multisig_prover
                    .clone(),
                &request,
            )
            .await;

        let mut sleep_duration = 5;
        if let Err(e) = res {
            if e.to_string()
                .contains("ticket count threshold has not been reached")
            {
                debug!("Ticket count threshold has not been reached. Ignoring.");
            } else {
                error!("Failed to broadcast XRPL Ticket Create request: {:?}", e);
            }
        } else {
            info!("Ticket create submitted: {}", res.unwrap_or_default());
            sleep_duration = 60; // that's usually how long it takes for the tickets to be confirmed
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(sleep_duration)).await;
    }

    pub async fn run(&self) {
        loop {
            info!("XrplTicketCreator is alive.");
            self.work().await;
        }
    }
}
