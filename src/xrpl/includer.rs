use std::sync::Arc;

use crate::config::NetworkConfig;
use crate::error::BroadcasterError;
use crate::gmp_api::GmpApi;
use crate::includer::Includer;

use super::broadcaster::XRPLBroadcaster;
use super::client::XRPLClient;
use super::refund_manager::XRPLRefundManager;

pub struct XrplIncluder {}

impl XrplIncluder {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new<'a>(
        config: NetworkConfig,
        gmp_api: Arc<GmpApi>,
    ) -> error_stack::Result<
        Includer<XRPLBroadcaster, Arc<XRPLClient>, XRPLRefundManager>,
        BroadcasterError,
    > {
        let client =
            Arc::new(XRPLClient::new(config.xrpl_rpc.as_str(), 3).map_err(|e| {
                error_stack::report!(BroadcasterError::GenericError(e.to_string()))
            })?);

        let broadcaster = XRPLBroadcaster::new(Arc::clone(&client))
            .map_err(|e| e.attach_printable("Failed to create XRPLBroadcaster"))?;

        let secrets = config.includer_secrets.split(",").collect::<Vec<&str>>();
        let addresses = config
            .refund_manager_addresses
            .split(",")
            .collect::<Vec<&str>>();
        let instance_id = std::env::var("INSTANCE_ID")
            .expect("INSTANCE_ID is not set")
            .parse::<usize>()
            .expect("Invalid instance id");
        if instance_id >= secrets.len() || instance_id >= addresses.len() {
            return Err(error_stack::report!(BroadcasterError::GenericError(
                "Instance id out of bounds".to_string()
            )));
        }
        let secret = secrets[instance_id];
        let address = addresses[instance_id];
        let refund_manager =
            XRPLRefundManager::new(Arc::clone(&client), address.to_string(), secret.to_string())
                .map_err(|e| error_stack::report!(BroadcasterError::GenericError(e.to_string())))?;

        let includer = Includer {
            chain_client: client,
            broadcaster,
            refund_manager,
            gmp_api,
        };

        Ok(includer)
    }
}
