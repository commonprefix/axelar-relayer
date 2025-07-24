/*!

Reads from the TON blockchain and adds transactions to a queue.

*/

use super::client::RestClient;
use crate::ton_trace::{AtomicUpsert, TONTrace};
use relayer_base::database::Database;
use relayer_base::error::SubscriberError;
use relayer_base::subscriber::{ChainTransaction, TransactionPoller};
use ton_types::ton_types::Trace;
use tonlib_core::TonAddress;
use tracing::{debug, info, warn};

pub struct TONSubscriber<DB: Database, TM: AtomicUpsert, CL: RestClient> {
    client: CL,
    latest_lt: i64,
    db: DB,
    context: String,
    chain_name: String,
    ton_trace_model: TM,
}

impl<DB: Database, TM: AtomicUpsert, CL: RestClient> TONSubscriber<DB, TM, CL> {
    pub async fn new(
        client: CL,
        db: DB,
        context: String,
        chain_name: String,
        ton_trace_model: TM,
    ) -> Result<Self, SubscriberError> {
        let latest_lt = db
            .get_latest_height(&chain_name, &context)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))?
            .unwrap_or(-1);

        if latest_lt != -1 {
            info!(
                "TON Subscriber for {}: starting from ledger index: {}",
                context, latest_lt
            );
        }
        Ok(TONSubscriber {
            client,
            latest_lt,
            db,
            context,
            chain_name,
            ton_trace_model,
        })
    }

    async fn store_latest_height(&mut self) -> Result<(), SubscriberError> {
        self.db
            .store_latest_height(&self.chain_name, &self.context, self.latest_lt)
            .await
            .map_err(|e| SubscriberError::GenericError(e.to_string()))
    }
}

impl<DB: Database, TM: AtomicUpsert, CL: RestClient> TransactionPoller
    for TONSubscriber<DB, TM, CL>
{
    type Transaction = Trace;
    type Account = TonAddress;

    fn make_queue_item(&mut self, tx: Self::Transaction) -> ChainTransaction {
        ChainTransaction::TON(Box::new(tx))
    }

    async fn poll_account(
        &mut self,
        account_id: TonAddress,
    ) -> Result<Vec<Self::Transaction>, anyhow::Error> {
        let start_lt = if self.latest_lt == -1 {
            None
        } else {
            Some(self.latest_lt)
        };

        let traces = self
            .client
            .get_traces_for_account(Some(account_id.clone()), None, start_lt)
            .await?;

        info!("Got {} traces for account {}", traces.len(), account_id);

        let max_lt = traces.iter().map(|trace| trace.end_lt).max();

        if max_lt.is_some() {
            self.latest_lt = max_lt.unwrap_or(0);
            if let Err(err) = self.store_latest_height().await {
                warn!("{:?}", err);
            }
        }

        let mut unseen_traces: Vec<Trace> = Vec::new();

        for trace in traces {
            let trace_model = TONTrace::from(&trace);
            if self
                .ton_trace_model
                .upsert_and_return_if_changed(trace_model)
                .await?
                .is_some()
                && !trace.is_incomplete
            {
                debug!("Trace {} added from account {}", trace.trace_id, account_id);
                unseen_traces.push(trace);
            } else if trace.is_incomplete {
                info!("Trace {} is incomplete, skipping", trace.trace_id);
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
    use crate::test_utils::fixtures::fixture_traces;
    use crate::ton_trace::MockAtomicUpsert;
    use mockall::predicate::eq;
    use relayer_base::database::MockDatabase;

    #[tokio::test]
    async fn test_subscriber_no_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(None) }));

        let subscriber = TONSubscriber::new(
            MockRestClient::new(),
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
            MockAtomicUpsert::new(),
        )
        .await
        .expect("TONSubscriber should be created successfully");

        assert_eq!(subscriber.latest_lt, -1);
    }

    #[tokio::test]
    async fn test_subscriber_with_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(Some(12345)) }));

        let subscriber = TONSubscriber::new(
            MockRestClient::new(),
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
            MockAtomicUpsert::new(),
        )
        .await
        .expect("TONSubscriber should be created successfully");

        assert_eq!(subscriber.latest_lt, 12345);
    }

    #[tokio::test]
    async fn test_poll_account_with_init_height() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .with(eq("test-chain"), eq("test-context"))
            .returning(|_, _| Box::pin(async { Ok(Some(12345)) }));

        let mut mock_client = MockRestClient::new();

        let expected_traces = fixture_traces();

        mock_client
            .expect_get_traces_for_account()
            .withf(|account, _, start_lt| {
                account.clone().unwrap().to_string()
                    == "EQCqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqseb"
                    && *start_lt == Some(12345)
            })
            .returning(move |_, _, _| {
                let txs = expected_traces.clone();
                Ok(txs)
            });
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

    #[tokio::test]
    async fn test_poll_account_trace_seen() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_latest_height()
            .returning(|_, _| Box::pin(async { Ok(Some(0)) }));

        mock_db
            .expect_store_latest_height()
            .returning(|_, _, _| Box::pin(async { Ok(()) }));

        let mut mock_client = MockRestClient::new();
        let trace1 = sample_trace("trace_1", 1, 2);
        let trace2 = sample_trace("trace_2", 3, 4);
        let trace3 = sample_trace("trace_3", 3, 4);
        let traces = vec![trace1, trace2, trace3];

        mock_client
            .expect_get_traces_for_account()
            .returning(move |_, _, _| Ok(traces.clone()));

        let mut mock_upsert = MockAtomicUpsert::new();
        mock_upsert
            .expect_upsert_and_return_if_changed()
            .returning(|trace| {
                Box::pin(async move {
                    if trace.trace_id == "trace_2" {
                        return Ok(None);
                    }
                    Ok(Some(trace))
                })
            });

        let mut subscriber = TONSubscriber::new(
            mock_client,
            mock_db,
            "test-context".to_string(),
            "test-chain".to_string(),
            mock_upsert,
        )
        .await
        .unwrap();

        let address =
            TonAddress::from_base64_url("EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c")
                .unwrap();

        let result = subscriber.poll_account(address).await.unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|t| t.trace_id == "trace_1"));
        assert!(result.iter().any(|t| t.trace_id == "trace_3"));
    }
}
