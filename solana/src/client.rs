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

const LIMIT: usize = 6;

pub trait SolanaClientTrait: Send + Sync {
    fn inner(&self) -> &RpcClient;

    fn get_transaction_by_signature(
        &self,
        commitment: Option<CommitmentConfig>,
        signature: Signature,
    ) -> impl Future<Output = Result<EncodedConfirmedTransactionWithStatusMeta, anyhow::Error>>;

    fn get_transactions_for_account(
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
    // before and until are exclusive
    // before is the chronologically latest transaction
    // until is the chronologically earliest transaction
    async fn get_transactions_for_account(
        &self,
        commitment: Option<CommitmentConfig>,
        address: &Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
    ) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, anyhow::Error> {
        let mut retries = 0;
        let mut delay = Duration::from_millis(500);
        let mut txs = vec![];
        let mut before_sig = before;
        let until_sig = until;

        // NOT READY YET, NEEDS PAGE LOGIC IMPLEMENTED + SCALABILITY ISSUES

        loop {
            // Config needs to be inside the loop because it does not implement clone
            let config = GetConfirmedSignaturesForAddress2Config {
                commitment,
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
                        info!("No more signatures to fetch");
                        return Ok(txs);
                    }

                    for status_with_signature in response.clone() {
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

                    // If we have less than LIMIT txs, we can return since there are no more pages
                    if response.len() < LIMIT {
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use dotenv::dotenv;
    use relayer_base::config::config_from_yaml;

    use crate::config::SolanaConfig;

    use super::*;

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
    //         .get_transaction_by_signature(Some(CommitmentConfig::confirmed()), signature)
    //         .await
    //         .unwrap();

    //     // println!("{:?}", transaction);
    //     println!("{:?}", transaction.transaction.meta.unwrap().log_messages);
    // }

    #[tokio::test]
    async fn test_get_transactions_for_account() {
        dotenv().ok();
        let network = std::env::var("NETWORK").expect("NETWORK must be set");
        let config: SolanaConfig = config_from_yaml(&format!("config.{}.yaml", network)).unwrap();

        let solana_client: SolanaClient =
            SolanaClient::new(&config.solana_rpc, CommitmentConfig::confirmed(), 3).unwrap();

        //let signature = Signature::from_str("viT9BuyqLeWy2jUwpHV7uurjvuq8PoDjC2aBPEHCDM5jhSsgZghXdzN36rdV1n35k8TazcxD5yLmhxLWMZmRCVc").unwrap();
        //let signature = Signature::from_str("5Pg6SHHKCBEz4yHtnsiK7EtTvnPk31WQ9Adh48XhwcDv7ghwLY4ADvTneq3bw64osqZwjwehVRBrKwDG2XNzrvFB").unwrap();
        let transactions = solana_client
            .get_transactions_for_account(
                Some(CommitmentConfig::confirmed()),
                &Pubkey::from_str("DaejccUfXqoAFTiDTxDuMQfQ9oa6crjtR9cT52v1AvGK").unwrap(),
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(transactions.len(), 5);

        for tx in transactions {
            println!("--------------------------------");
            println!("{:?}", tx.transaction.transaction);
        }
    }
}
