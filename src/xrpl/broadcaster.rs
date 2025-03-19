use std::sync::Arc;

use tracing::{debug, warn};
use xrpl_api::{ResultCategory, SubmitRequest, SubmitResponse, TxRequest};

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

fn log_and_return_error(
    response: &SubmitResponse,
    message_id: Option<String>,
    source_chain: Option<String>,
) -> Result<
    (
        Result<String, BroadcasterError>,
        Option<String>,
        Option<String>,
    ),
    BroadcasterError,
> {
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
        let tx = response.tx_json.clone();
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

        let response_category = response.engine_result.category();
        if response_category == ResultCategory::Tec || response_category == ResultCategory::Tes {
            let tx_hash = tx.common().hash.as_ref().ok_or_else(|| {
                BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
            })?;
            Ok((Ok(tx_hash.clone()), message_id, source_chain))
        } else if response_category == ResultCategory::Tef {
            let tx_hash = tx.common().hash.as_ref().ok_or_else(|| {
                BroadcasterError::RPCCallFailed("Transaction hash not found".to_string())
            })?;
            let req = TxRequest::new(tx_hash);
            match self.client.call(req).await {
                Ok(_) => {
                    warn!("Transaction already submitted: {:?}", tx_hash);
                    Ok((Ok(tx_hash.clone()), message_id, source_chain))
                }
                Err(_) => log_and_return_error(&response, message_id, source_chain),
            }
        } else {
            log_and_return_error(&response, message_id, source_chain)
        }
    }
}
