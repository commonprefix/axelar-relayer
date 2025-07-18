use std::sync::Arc;

use tracing::{debug, error, info};

use relayer_base::{
    gmp_api::{gmp_types::BroadcastRequest, GmpApi},
};
use xrpl_multisig_prover;
use crate::config::XRPLConfig;

pub struct XrplTicketCreator {
    gmp_api: Arc<GmpApi>,
    config: XRPLConfig,
}

impl XrplTicketCreator {
    pub fn new(gmp_api: Arc<GmpApi>, config: XRPLConfig) -> Self {
        Self { gmp_api, config }
    }

    async fn work(&self) {
        let request = BroadcastRequest::Generic(
            serde_json::to_value(xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate).unwrap(),
        );

        let res = self
            .gmp_api
            .post_broadcast(
                self.config.common_config.axelar_contracts.chain_multisig_prover.clone(),
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
            info!("Ticket create submitted: {}", res.unwrap());
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
