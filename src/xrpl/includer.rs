use std::sync::Arc;

use crate::config::Config;
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
        config: Config,
        gmp_api: Arc<GmpApi>,
        redis_pool: r2d2::Pool<redis::Client>,
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

        let refund_manager = XRPLRefundManager::new(Arc::clone(&client), config, redis_pool)
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
