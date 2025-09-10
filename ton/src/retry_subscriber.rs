/*!

Retries any incomplete traces

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

    fn transaction_id(&self, tx: &Self::Transaction) -> Option<String> {
        Some(tx.trace_id.to_string())
    }

    fn account_id(&self, account: &Self::Account) -> Option<String> {
        Some(account.to_hex())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockRestClient;
    use crate::ton_trace::{MockAtomicUpsert, MockRetriable, TONTrace};
    use mockall::predicate::*;

    // To simplify tests, we use a single model that implements both Retriable and AtomicUpsert
    struct MockTraceModel {
        retriable: MockRetriable,
        atomic_upsert: MockAtomicUpsert,
    }

    impl MockTraceModel {
        fn new() -> Self {
            Self {
                retriable: MockRetriable::new(),
                atomic_upsert: MockAtomicUpsert::new(),
            }
        }
    }

    impl Retriable for MockTraceModel {
        async fn fetch_retry(&self, limit: u32) -> anyhow::Result<Vec<TONTrace>> {
            self.retriable.fetch_retry(limit).await
        }

        async fn decrease_retry(&self, tx: TONTrace) -> anyhow::Result<()> {
            self.retriable.decrease_retry(tx).await
        }
    }

    impl AtomicUpsert for MockTraceModel {
        async fn upsert_and_return_if_changed(
            &self,
            tx: TONTrace,
        ) -> anyhow::Result<Option<TONTrace>> {
            self.atomic_upsert.upsert_and_return_if_changed(tx).await
        }
    }

    fn sample_trace(id: &str, start_lt: i64, end_lt: i64) -> Trace {
        Trace {
            trace_id: id.to_string(),
            is_incomplete: false,
            start_lt,
            end_lt,
            transactions: vec![],
        }
    }

    #[test]
    fn test_transaction_id() {
        let mock_client = MockRestClient::new();
        let mock_trace_model = MockTraceModel::new();

        let subscriber = RetryTONSubscriber {
            client: mock_client,
            ton_trace_model: mock_trace_model,
        };

        let trace = sample_trace("test-trace-id", 1, 2);
        let result = subscriber.transaction_id(&trace);

        assert_eq!(result, Some("test-trace-id".to_string()));
    }

    #[test]
    fn test_account_id() {
        let mock_client = MockRestClient::new();
        let mock_trace_model = MockTraceModel::new();

        let subscriber = RetryTONSubscriber {
            client: mock_client,
            ton_trace_model: mock_trace_model,
        };

        let address =
            TonAddress::from_base64_url("EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c")
                .unwrap();
        let result = subscriber.account_id(&address);

        assert_eq!(result, Some(address.to_hex()));
    }
}
