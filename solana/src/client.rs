use std::{future::Future, str::FromStr, time::Duration};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};

use relayer_base::error::ClientError;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use tracing::{debug, info};

const LIMIT: usize = 10;

pub trait SolanaClientTrait: Send + Sync {
    fn inner(&self) -> &RpcClient;

    fn get_transaction_by_signature(
        &self,
        commitment: Option<CommitmentConfig>,
        signature: Signature,
    ) -> impl Future<Output = Result<EncodedConfirmedTransactionWithStatusMeta, anyhow::Error>>;

    fn get_account_logs(
        &self,
        commitment: Option<CommitmentConfig>,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> impl Future<Output = Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, anyhow::Error>>;
}

pub struct SolanaClient {
    client: RpcClient,
    max_retries: usize,
}

impl SolanaClient {
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

impl SolanaClientTrait for SolanaClient {
    fn inner(&self) -> &RpcClient {
        &self.client
    }

    async fn get_transaction_by_signature(
        &self,
        commitment: Option<CommitmentConfig>,
        signature: Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta, anyhow::Error> {
        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Binary),
            commitment,
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
                Ok(response) => return Ok(response),
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
    // To simulate "after" we need to set before to None and until to the signature that would be the "after" signature
    async fn get_account_logs(
        &self,
        commitment: Option<CommitmentConfig>,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, anyhow::Error> {
        let mut retries = 0;
        let mut delay = Duration::from_millis(500);
        let mut txs = vec![];

        // NOT READY YET, NEEDS PAGE LOGIC IMPLEMENTED + SCALABILITY ISSUES

        loop {
            // Config needs to be inside the loop because it does not implement clone
            let config = GetConfirmedSignaturesForAddress2Config {
                commitment,
                limit: Some(LIMIT),
                before,
                until,
            };
            // This function returns the signatures. We then need to get the transactions for each signature
            match self
                .client
                .get_signatures_for_address_with_config(address, config)
                .await
            {
                Ok(response) => {
                    if response.is_empty() {
                        info!("No more signatures to fetch");
                        return Ok(txs);
                    }

                    for status_with_signature in response {
                        // TODO: Spawn tasks for the loop. Can the same client be used for all the tasks?
                        // Also, this is a "double" retry mechanism. Maybe it's unnecessary
                        let tx = self
                            .get_transaction_by_signature(
                                commitment,
                                Signature::from_str(&status_with_signature.signature)?,
                            )
                            .await?;
                        txs.push(tx);
                    }
                    // if txs.len() < LIMIT {
                    //     debug!("Fetched less than {} txs. Stopping", LIMIT);
                    //     return Ok(txs);
                    // }

                    // call next page instead of returning
                    return Ok(txs);
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use dotenv::dotenv;
    use relayer_base::config::config_from_yaml;

    use crate::config::SolanaConfig;

    use super::*;

    #[tokio::test]
    async fn test_get_transaction_by_signature() {
        dotenv().ok();
        let network = std::env::var("NETWORK").expect("NETWORK must be set");
        let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

        let solana_client: SolanaClient =
            SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3).unwrap();

        //let signature = Signature::from_str("viT9BuyqLeWy2jUwpHV7uurjvuq8PoDjC2aBPEHCDM5jhSsgZghXdzN36rdV1n35k8TazcxD5yLmhxLWMZmRCVc").unwrap();
        let signature = Signature::from_str("5Pg6SHHKCBEz4yHtnsiK7EtTvnPk31WQ9Adh48XhwcDv7ghwLY4ADvTneq3bw64osqZwjwehVRBrKwDG2XNzrvFB").unwrap();
        let transaction = solana_client
            .get_transaction_by_signature(Some(CommitmentConfig::confirmed()), signature)
            .await
            .unwrap();

        // println!("{:?}", transaction);
        println!("{:?}", transaction.transaction.meta.unwrap().log_messages);
    }
}
