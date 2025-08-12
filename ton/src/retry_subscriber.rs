/*!

Reads from the TON blockchain and adds transactions to a queue.

*/

use super::client::RestClient;
use crate::ton_trace::{AtomicUpsert, Retriable, TONTrace};
use relayer_base::error::SubscriberError;
use relayer_base::subscriber::{ChainTransaction, TransactionPoller};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use ton_types::ton_types::Trace;
use tonlib_core::TonAddress;
use tracing::{debug, info, warn};

pub struct RetryTONSubscriber<TM: Retriable + AtomicUpsert, CL: RestClient> {
    client: CL,
    ton_trace_model: TM,
}

impl<TM: Retriable + AtomicUpsert, CL: RestClient> RetryTONSubscriber<TM, CL> {
    pub async fn new(client: CL, ton_trace_model: TM) -> Result<Self, SubscriberError> {
        Ok(RetryTONSubscriber {
            client,
            ton_trace_model,
        })
    }
}

impl<TM: Retriable + AtomicUpsert, CL: RestClient> TransactionPoller
    for RetryTONSubscriber<TM, CL>
{
    type Transaction = Trace;
    type Account = TonAddress;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::TON(Box::new(tx))
    }

    #[tracing::instrument(skip(self))]
    async fn poll_account(
        &mut self,
        account_id: TonAddress,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        sleep(Duration::from_secs(5)).await;

        let traces = self.ton_trace_model.fetch_retry(100).await?;

        if traces.is_empty() {
            debug!("No retry traces to process");
            return Ok(Vec::new());
        }

        let mut retry_map: HashMap<String, i32> = HashMap::new();
        let mut trace_ids: Vec<String> = Vec::with_capacity(traces.len());

        info!("Processing {} retry traces: {:?}", traces.len(), trace_ids);

        for trace in &traces {
            trace_ids.push(trace.trace_id.clone());
            retry_map.insert(trace.trace_id.clone(), trace.retries);
        }

        let traces = self
            .client
            .get_traces_for_account(None, Some(trace_ids), None)
            .await?;

        let mut unseen_traces: Vec<Trace> = Vec::new();

        for trace in traces {
            let trace_model = TONTrace::from(&trace);
            if trace_model.is_incomplete {
                self.ton_trace_model.decrease_retry(trace_model).await?;
                let retry = retry_map.get(&trace.trace_id).copied().unwrap_or(0);
                if retry <= 1 {
                    warn!(
                        "Trace {} is still incomplete and retry count is {}, skipping",
                        trace.trace_id, retry
                    );
                } else {
                    info!("Trace {} still incomplete, skipping", trace.trace_id);
                }
            } else if self
                .ton_trace_model
                .upsert_and_return_if_changed(trace_model)
                .await?
                .is_some()
                && !trace.is_incomplete
            {
                debug!(
                    "Trace {} added from account {} after retry",
                    trace.trace_id, account_id
                );
                unseen_traces.push(trace);
            } else {
                info!("Trace {} already seen, skipping", trace.trace_id);
            }
        }

        Ok(unseen_traces)
    }

    async fn poll_tx(&mut self, _tx_hash: String) -> Result<Self::Transaction, anyhow::Error> {
        unimplemented!();
    }
}
