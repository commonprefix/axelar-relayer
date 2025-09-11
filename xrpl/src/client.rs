use std::{future::Future, time::Duration};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info};
use xrpl_api::{
    LedgerIndex, LedgerObject, ObjectType, Request, RequestPagination, RetrieveLedgerSpec, Ticket,
    Transaction,
};
use xrpl_types::AccountId;

use relayer_base::error::ClientError;
use xrpl_http_client;

const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg_attr(test, mockall::automock)]
pub trait XRPLClientTrait: Send + Sync {
    fn inner(&self) -> &xrpl_http_client::Client;

    fn call<Req>(
        &self,
        request: Req,
    ) -> impl Future<Output = Result<Req::Response, xrpl_http_client::error::Error>>
    where
        Req: Request + Serialize + std::fmt::Debug + std::clone::Clone + Send + 'static,
        Req::Response: DeserializeOwned + Send + 'static;

    fn get_transaction_by_id(
        &self,
        tx_id: String,
    ) -> impl Future<Output = Result<Transaction, anyhow::Error>>;

    fn get_transactions_for_account(
        &self,
        account: &AccountId,
        ledger_index_min: u32,
    ) -> impl Future<Output = Result<Vec<Transaction>, anyhow::Error>>;

    fn get_available_tickets_for_account(
        &self,
        account: &AccountId,
    ) -> impl Future<Output = Result<Vec<Ticket>, anyhow::Error>>;
}

pub struct XRPLClient {
    client: xrpl_http_client::Client,
    max_retries: usize,
}

impl XRPLClient {
    pub fn new(url: &str, max_retries: usize) -> Result<Self, ClientError> {
        let http_client = reqwest::ClientBuilder::new()
            .connect_timeout(DEFAULT_RPC_TIMEOUT)
            .timeout(DEFAULT_RPC_TIMEOUT)
            .build()
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            client: xrpl_http_client::Client::builder()
                .base_url(url)
                .http_client(http_client)
                .build(),
            max_retries,
        })
    }
}

impl XRPLClientTrait for XRPLClient {
    fn inner(&self) -> &xrpl_http_client::Client {
        &self.client
    }

    #[tracing::instrument(skip(self))]
    async fn call<Req>(&self, request: Req) -> Result<Req::Response, xrpl_http_client::error::Error>
    where
        Req: Request + Serialize + std::fmt::Debug + std::clone::Clone + Send + 'static,
        Req::Response: DeserializeOwned + Send + 'static,
    {
        let mut retries = 0;
        let mut delay = Duration::from_millis(500);

        loop {
            match self.client.call(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if retries >= self.max_retries {
                        return Err(e);
                    }

                    debug!(
                        "RPC call ({}) failed (retry {}/{}): {}. Retrying in {:?}...",
                        request.method(),
                        retries + 1,
                        self.max_retries,
                        e,
                        delay
                    );

                    tokio::time::sleep(delay).await;
                    retries += 1;
                    delay = delay.mul_f32(2.0);
                }
            }
        }
    }

    async fn get_transaction_by_id(&self, tx_id: String) -> Result<Transaction, anyhow::Error> {
        let request = xrpl_api::TxRequest::new(&tx_id);
        let res = self.call(request).await;
        let response = res.map_err(|e| anyhow!("Error getting txs: {:?}", e.to_string()))?;
        Ok(response.tx)
    }

    async fn get_transactions_for_account(
        &self,
        account: &AccountId,
        ledger_index_min: u32,
    ) -> Result<Vec<Transaction>, anyhow::Error> {
        let mut all_transactions = Vec::new();
        let mut marker = None;
        let mut request = xrpl_api::AccountTxRequest {
            account: account.to_address(),
            forward: Some(true),
            ledger_index_min: Some(ledger_index_min.to_string()),
            ledger_index_max: Some((-1).to_string()),
            pagination: RequestPagination {
                limit: Some(100),
                ..Default::default()
            },
            ..Default::default()
        };

        loop {
            request.pagination.marker = marker;
            let res = self.call(request.clone()).await;
            let response = res.map_err(|e| anyhow!("Error getting txs: {:?}", e.to_string()))?;

            // Add transactions from this page to our collection
            all_transactions.extend(response.transactions.iter().map(|tx| tx.tx.clone()));

            // Check if there are more pages
            marker = response.pagination.marker;
            if marker.is_none() {
                break;
            } else {
                info!("More pages to fetch");
            }
        }

        Ok(all_transactions)
    }

    async fn get_available_tickets_for_account(
        &self,
        account: &AccountId,
    ) -> Result<Vec<Ticket>, anyhow::Error> {
        let request = xrpl_api::AccountObjectsRequest {
            account: account.to_address(),
            object_type: Some(ObjectType::Ticket),
            pagination: RequestPagination {
                // the max limit is 400
                // The limit refers to all the objects before applying the object_type filter,
                // so setting that to 250 (max number of tickets) would miss tickets if the account had 250 tickets + some other objects.
                limit: Some(400),
                ..Default::default()
            },
            ledger_spec: RetrieveLedgerSpec {
                ledger_index: Some(LedgerIndex::Validated),
                ..Default::default()
            },
        };
        let res = self.call(request.clone()).await;
        let response = res.map_err(|e| anyhow!("Error getting tickets: {:?}", e.to_string()))?;
        let tickets = response
            .account_objects
            .into_iter()
            .filter_map(|o| match o {
                LedgerObject::Ticket(ticket) => Some(ticket),
                _ => None,
            })
            .collect();
        Ok(tickets)
    }
}
