use std::{
    future::{ready, Future},
    pin::Pin,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;

use futures::{
    future::{join_all, Either},
    lock::Mutex,
};
use futures_util::{future::BoxFuture, stream::BoxStream};
use relayer_base::error::ClientError;
use solana_client::{
    rpc_client::GetConfirmedSignaturesForAddress2Config,
    rpc_config::{RpcTransactionConfig, RpcTransactionLogsConfig, RpcTransactionLogsFilter},
    rpc_response::RpcLogsResponse,
};
use solana_pubsub_client::nonblocking::pubsub_client::PubsubClient;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::response::Response as RpcResponse;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::UiTransactionEncoding;
use solana_types::solana_types::SolanaTransaction;
use tracing::{debug, error, info};

const LIMIT: usize = 10;

// Match the nonblocking PubsubClient logs_subscribe return type
// (BoxStream<'a, RpcResponse<RpcLogsResponse>>, UnsubscribeFn)
// UnsubscribeFn = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>
pub type LogsSubscription<'a> = BoxStream<'a, RpcResponse<RpcLogsResponse>>;

type UnsubFn =
    Box<dyn FnOnce() -> Pin<Box<dyn core::future::Future<Output = ()> + Send>> + Send + 'static>;

pub trait SolanaRpcClientTrait: Send + Sync {
    fn inner(&self) -> &RpcClient;

    fn get_transaction_by_signature(
        &self,
        signature: Signature,
    ) -> impl Future<Output = Result<SolanaTransaction, anyhow::Error>>;

    fn get_transactions_for_account(
        &self,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> impl Future<Output = Result<Vec<SolanaTransaction>, anyhow::Error>>;
}

pub trait SolanaStreamClientTrait: Send + Sync {
    fn inner(&self) -> &PubsubClient;

    fn logs_subscriber(
        &self,
        account: String,
    ) -> impl Future<Output = Result<LogsSubscription<'_>, anyhow::Error>>;

    fn unsubscribe(&self) -> impl Future<Output = ()>;
}

pub struct SolanaRpcClient {
    client: RpcClient,
    max_retries: usize,
}

pub struct SolanaStreamClient {
    client: PubsubClient,
    commitment: CommitmentConfig,
    max_retries: usize,
    unsub: Mutex<Option<UnsubFn>>,
}

impl SolanaRpcClient {
    pub fn new(
        url: &str,
        commitment: CommitmentConfig,
        max_retries: usize,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            client: RpcClient::new_with_commitment(url.to_string(), commitment),
            max_retries,
        })
    }
}

impl SolanaRpcClientTrait for SolanaRpcClient {
    fn inner(&self) -> &RpcClient {
        &self.client
    }

    async fn get_transaction_by_signature(
        &self,
        signature: Signature,
    ) -> Result<SolanaTransaction, anyhow::Error> {
        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Binary),
            commitment: Some(self.client.commitment()),
            max_supported_transaction_version: None,
        };

        let mut retries = 0;
        let mut delay = Duration::from_millis(500);

        loop {
            match self
                .client
                .get_transaction_with_config(&signature, config)
                .await
            {
                Ok(response) => {
                    let maybe_meta = &response.transaction.meta;
                    let solana_tx = SolanaTransaction {
                        signature: signature,
                        transaction: serde_json::to_string(&response.transaction)?,
                        timestamp: None,
                        logs: match maybe_meta {
                            Some(meta) => meta.log_messages.clone().into(),
                            None => None,
                        },
                        slot: response.slot,
                        cost_in_lamports: match maybe_meta {
                            Some(meta) => meta.fee,
                            None => 0,
                        },
                    };
                    return Ok(solana_tx);
                }
                Err(e) => {
                    if retries >= self.max_retries {
                        return Err(anyhow!(
                            "Failed to get transaction with config: {:?}",
                            e.to_string()
                        ));
                    }

                    debug!(
                        "RPC call ({}) failed (retry {}/{}): {}. Retrying in {:?}...",
                        "get_transaction_with_config",
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

    // Traverses the account's tx history backwards
    // before and until are exclusive
    // before is the chronologically latest transaction
    // until is the chronologically earliest transaction
    async fn get_transactions_for_account(
        &self,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> Result<Vec<SolanaTransaction>, anyhow::Error> {
        let mut retries = 0;
        let mut delay = Duration::from_millis(500);
        let mut txs: Vec<SolanaTransaction> = vec![];
        let mut before_sig = before;
        let until_sig = until;

        // NOT READY YET, NEEDS PAGE LOGIC IMPLEMENTED + SCALABILITY ISSUES

        loop {
            // Config needs to be inside the loop because it does not implement clone
            let config = GetConfirmedSignaturesForAddress2Config {
                commitment: Some(self.client.commitment()),
                limit: Some(LIMIT),
                before: before_sig,
                until: until_sig,
            };
            // This function returns the signatures. We then need to get the transactions for each signature
            match self
                .client
                .get_signatures_for_address_with_config(address, config)
                .await
            {
                Ok(response) => {
                    // edge case where last page had exactly LIMIT txs and we did one extra request
                    if response.is_empty() {
                        info!("No more signatures to fetch, empty response");
                        return Ok(txs);
                    }

                    debug!("Fetched {} signatures", response.len());

                    let futures = response.iter().map(|status_with_signature| {
                        let sig_str = status_with_signature.signature.clone();

                        match Signature::from_str(&sig_str) {
                            Ok(sig) => Either::Left(self.get_transaction_by_signature(sig)),
                            Err(e) => {
                                Either::Right(ready(Err(anyhow!("Error parsing signature: {}", e))))
                            }
                        }
                    });

                    let results = join_all(futures).await;

                    for res in results {
                        match res {
                            Ok(tx) => {
                                txs.push(tx);
                            }
                            Err(e) => {
                                error!("Error fetching tx: {:?}", e);
                                return Err(anyhow!("Error fetching tx: {:?}", e));
                            }
                        }
                    }

                    // If we have less than LIMIT txs, we can return since there are no more pages
                    if response.len() < LIMIT {
                        info!(
                            "No more signatures to fetch, fetched {} and limit is {}",
                            response.len(),
                            LIMIT
                        );
                        return Ok(txs);
                    }

                    let maybe_earliest_signature = response.last();
                    match maybe_earliest_signature {
                        Some(earliest_signature) => {
                            before_sig = Some(Signature::from_str(&earliest_signature.signature)?)
                        }
                        None => return Err(anyhow!("No earliest signature found")),
                    }
                }

                Err(e) => {
                    if retries >= self.max_retries {
                        return Err(anyhow!(
                            "Failed to get transaction with config: {:?}",
                            e.to_string()
                        ));
                    }

                    debug!(
                        "RPC call ({}) failed (retry {}/{}): {}. Retrying in {:?}...",
                        "get_transaction_with_config",
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
}

impl SolanaStreamClient {
    pub async fn new(
        url: &str,
        commitment: CommitmentConfig,
        max_retries: usize,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            client: PubsubClient::new(url)
                .await
                .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?,
            max_retries,
            commitment,
            unsub: Mutex::new(None),
        })
    }
}

impl SolanaStreamClientTrait for SolanaStreamClient {
    fn inner(&self) -> &PubsubClient {
        &self.client
    }

    async fn logs_subscriber(
        &self,
        account: String,
    ) -> Result<LogsSubscription<'_>, anyhow::Error> {
        let (sub, unsub) = self
            .client
            .logs_subscribe(
                RpcTransactionLogsFilter::Mentions(vec![account]),
                RpcTransactionLogsConfig {
                    commitment: Some(self.commitment),
                },
            )
            .await
            .map_err(|e| anyhow!(e.to_string()))?;
        *self.unsub.lock().await = Some(unsub);
        Ok(sub)
    }

    async fn unsubscribe(&self) {
        if let Some(unsub) = self.unsub.lock().await.take() {
            unsub().await; // consume the FnOnce
        }
    }
}

#[cfg(test)]
mod tests {
    // use std::str::FromStr;

    // use dotenv::dotenv;
    // use relayer_base::config::config_from_yaml;

    // use crate::config::SolanaConfig;

    // use super::*;

    // #[tokio::test]
    // async fn test_get_transaction_by_signature() {
    //     dotenv().ok();
    //     let network = std::env::var("NETWORK").expect("NETWORK must be set");
    //     let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    //     let solana_client: SolanaClient =
    //         SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3).unwrap();

    //     //let signature = Signature::from_str("viT9BuyqLeWy2jUwpHV7uurjvuq8PoDjC2aBPEHCDM5jhSsgZghXdzN36rdV1n35k8TazcxD5yLmhxLWMZmRCVc").unwrap();
    //     let signature = Signature::from_str("5Pg6SHHKCBEz4yHtnsiK7EtTvnPk31WQ9Adh48XhwcDv7ghwLY4ADvTneq3bw64osqZwjwehVRBrKwDG2XNzrvFB").unwrap();
    //     let transaction = solana_client
    //         .get_transaction_by_signature(signature)
    //         .await
    //         .unwrap();

    //     // println!("{:?}", transaction);
    //     println!("{:?}", transaction.1.transaction.meta.unwrap().log_messages);
    // }

    // #[tokio::test]
    // async fn test_get_transactions_for_account() {
    //     dotenv().ok();
    //     let network = std::env::var("NETWORK").expect("NETWORK must be set");
    //     let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

    //     let solana_client: SolanaClient =
    //         SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3).unwrap();

    //     //let signature = Signature::from_str("viT9BuyqLeWy2jUwpHV7uurjvuq8PoDjC2aBPEHCDM5jhSsgZghXdzN36rdV1n35k8TazcxD5yLmhxLWMZmRCVc").unwrap();
    //     //let signature = Signature::from_str("5Pg6SHHKCBEz4yHtnsiK7EtTvnPk31WQ9Adh48XhwcDv7ghwLY4ADvTneq3bw64osqZwjwehVRBrKwDG2XNzrvFB").unwrap();
    //     let transactions = solana_client
    //         .get_transactions_for_account(
    //             &Pubkey::from_str("9EHADvhP1vnYsk1XVYjJ4qpZ9jP33nHy84wo2CGnDDij").unwrap(),
    //             None,
    //             None,
    //         )
    //         .await
    //         .unwrap();

    //     println!("LENGTH: {:?}", transactions.len());

    //     //assert_eq!(transactions.len(), 18);

    //     for tx in transactions {
    //         println!("--------------------------------");
    //         println!("{:?}", tx.transaction);
    //     }
    // }
}
