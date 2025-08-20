use std::{future::Future, time::Duration};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, Serialize};

use relayer_base::error::ClientError;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use tracing::debug;

pub trait SolanaClientTrait: Send + Sync {
    fn inner(&self) -> &RpcClient;

    fn get_transaction_by_signature(
        &self,
        commitment: CommitmentConfig,
        signature: Signature,
    ) -> impl Future<Output = Result<EncodedConfirmedTransactionWithStatusMeta, anyhow::Error>>;

    fn get_transactions_for_account(
        &self,
        account: &Pubkey,
        start_slot: u64,
    ) -> impl Future<Output = Result<Vec<Transaction>, anyhow::Error>>;
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
        commitment: CommitmentConfig,
        signature: Signature,
    ) -> Result<EncodedConfirmedTransactionWithStatusMeta, anyhow::Error> {
        let config = RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Binary),
            commitment: Some(commitment),
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

    async fn get_transactions_for_account(
        &self,
        account: &Pubkey,
        start_slot: u64,
    ) -> Result<Vec<Transaction>, anyhow::Error> {
        Err(anyhow!("Not implemented"))
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

        let signature = Signature::from_str("viT9BuyqLeWy2jUwpHV7uurjvuq8PoDjC2aBPEHCDM5jhSsgZghXdzN36rdV1n35k8TazcxD5yLmhxLWMZmRCVc").unwrap();
        let transaction = solana_client
            .get_transaction_by_signature(CommitmentConfig::confirmed(), signature)
            .await
            .unwrap();

        println!("{:?}", transaction);
        assert_eq!(transaction.slot, 402385708);
    }
}
