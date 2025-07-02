use reqwest::Client;
use tracing::{error, info};
use xrpl_api::{AccountInfoRequest, Amount};

use relayer_base::config::Config;

use crate::client::XRPLClientTrait;

const XRP_TOPUP_AMOUNT: u64 = 1000;
const BALANCE_THRESHOLD: f64 = 500_000_000.0; // = 500 XRP

pub struct XRPLFunder<X: XRPLClientTrait> {
    request_client: Client,
    xrpl_client: X,
    config: Config,
}

impl<X: XRPLClientTrait> XRPLFunder<X> {
    pub fn new(config: Config, xrpl_client: X) -> Self {
        let request_client = Client::new();
        Self {
            request_client,
            xrpl_client,
            config,
        }
    }

    async fn top_up_account(&self, address: &str, amount: u64) -> Result<String, anyhow::Error> {
        let req = serde_json::json!({
            "destination": address,
            "xrpAmount": amount.to_string()
        });

        let resp = self
            .request_client
            .post(self.config.xrpl_faucet_url.clone())
            .json(&req)
            .send()
            .await;

        match resp {
            Ok(resp) => {
                let result: serde_json::Value =
                    resp.json().await.map_err(|e| anyhow::anyhow!(e))?;
                let transaction_hash = result["transactionHash"]
                    .as_str()
                    .ok_or(anyhow::anyhow!("transactionHash not found in {:?}", result))?;
                Ok(transaction_hash.to_string())
            }
            Err(e) => Err(anyhow::anyhow!("Error topping up account: {}", e)),
        }
    }

    pub async fn run(&self) {
        loop {
            for address in self.config.refund_manager_addresses.split(',') {
                let request = AccountInfoRequest {
                    account: address.to_string(),
                    ..AccountInfoRequest::default()
                };
                let request_result = self.xrpl_client.call(request).await;
                if let Ok(response) = request_result {
                    let maybe_balance = response.account_data.balance;
                    if let Some(drops) = maybe_balance {
                        let balance_drops = Amount::Drops(drops).size();

                        if balance_drops < BALANCE_THRESHOLD {
                            info!(
                                "Balance of {} is {}. Topping up with {} XRP",
                                address,
                                balance_drops / 1_000_000.0,
                                XRP_TOPUP_AMOUNT
                            );
                            let topup_result = self.top_up_account(address, XRP_TOPUP_AMOUNT).await;
                            match topup_result {
                                Ok(transaction_hash) => {
                                    info!("Top-up transaction hash: {}", transaction_hash);
                                }
                                Err(e) => {
                                    error!("{}", e);
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    }
}
