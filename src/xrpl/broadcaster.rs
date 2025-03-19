use std::sync::Arc;

use tracing::debug;
use xrpl_api::{ResultCategory, SubmitRequest};

use crate::{error::BroadcasterError, includer::Broadcaster, utils::extract_hex_xrpl_memo};

use super::client::XRPLClient;

pub struct XRPLBroadcaster {
    client: Arc<XRPLClient>,
}

impl XRPLBroadcaster {
    pub fn new(client: Arc<XRPLClient>) -> error_stack::Result<Self, BroadcasterError> {
        Ok(XRPLBroadcaster { client })
    }
}

impl Broadcaster for XRPLBroadcaster {
    async fn broadcast(
        &self,
        tx_blob: String,
    ) -> Result<
        (
            Result<String, BroadcasterError>,
            Option<String>,
            Option<String>,
        ),
        BroadcasterError,
    > {
        let req = SubmitRequest::new(tx_blob);
        let response = self
            .client
            .call(req)
            .await
            .map_err(|e| BroadcasterError::RPCCallFailed(e.to_string()))?;

        let mut message_id = None;
        let mut source_chain = None;
        let tx = response.tx_json;
        if let xrpl_api::Transaction::Payment(payment_transaction) = &tx {
            let memos = payment_transaction.common.memos.clone();
            message_id = Some(
                extract_hex_xrpl_memo(memos.clone(), "message_id")
                    .map_err(|e| BroadcasterError::GenericError(e.to_string()))?,
            );
            source_chain = Some(
                extract_hex_xrpl_memo(memos.clone(), "source_chain")
                    .map_err(|e| BroadcasterError::GenericError(e.to_string()))?,
            );
        }

        if response.engine_result.category() == ResultCategory::Tec
            || response.engine_result.category() == ResultCategory::Tes
        {
            // TODO: handle tx that has already been succesfully broadcast
            let tx_hash = tx.common().hash.as_ref().ok_or_else(|| {
                BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
            })?;
            Ok((Ok(tx_hash.clone()), message_id, source_chain))
        } else {
            debug!(
                "Transaction failed: {:?}: {}",
                response.engine_result.clone(),
                response.engine_result_message.clone()
            );
            Ok((
                Err(BroadcasterError::RPCCallFailed(format!(
                    "Transaction failed: {:?}: {}",
                    response.engine_result, response.engine_result_message
                ))),
                message_id,
                source_chain,
            ))
        }
    }
}
