use std::sync::Arc;

use tracing::{debug, error, info};

use crate::{
    config::NetworkConfig,
    gmp_api::{gmp_types::BroadcastRequest, GmpApi},
};

pub struct XrplTicketCreator {
    gmp_api: Arc<GmpApi>,
    config: NetworkConfig,
}

impl XrplTicketCreator {
    pub fn new(gmp_api: Arc<GmpApi>, config: NetworkConfig) -> Self {
        Self { gmp_api, config }
    }

    async fn work(&self) {
        let request = BroadcastRequest::Generic(
            serde_json::to_value(xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate).unwrap(),
        );

        let res = self
            .gmp_api
            .post_broadcast(
                self.config.axelar_contracts.xrpl_multisig_prover.clone(),
                &request,
            )
            .await;

        if let Err(e) = res {
            if e.to_string()
                .contains("ticket count threshold has not been reached")
            {
                debug!("Ticket count threshold has not been reached. Ignoring.");
            } else {
                error!("Failed to broadcast XRPL Ticket Create request: {:?}", e);
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await
    }

    pub async fn run(&self) {
        loop {
            info!("XrplTicketCreator is alive.");
            self.work().await;
        }
    }
}
