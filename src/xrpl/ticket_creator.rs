use std::sync::Arc;

use tracing::{debug, error, info};

use crate::{
    config::Config,
    gmp_api::{gmp_types::BroadcastRequest, GmpApi},
};

pub struct XrplTicketCreator {
    gmp_api: Arc<GmpApi>,
    config: Config,
}

impl XrplTicketCreator {
    pub fn new(gmp_api: Arc<GmpApi>, config: Config) -> Self {
        Self { gmp_api, config }
    }

    async fn work(&self) -> () {
        let request = BroadcastRequest::Generic(
            serde_json::to_value(xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate).unwrap(),
        );

        let res = self
            .gmp_api
            .post_broadcast(self.config.xrpl_multisig_prover_address.clone(), &request)
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

        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await
    }

    pub async fn run(&self) -> () {
        loop {
            info!("XrplTicketCreator is alive.");
            self.work().await;
        }
    }
}
