use crate::client::{AccountState, RestClient};
use tokio::time::{sleep, Duration};
use tonlib_core::TonAddress;
use tracing::{error, info};

#[derive(Debug, PartialEq)]
pub enum AccountCheckStatus {
    Valid,
    Inactive,
    InsufficientBalance { balance: u64, required: u64 },
    InvalidBalanceFormat,
}

pub fn check_account_status(account: &AccountState, min_balance: u64) -> AccountCheckStatus {
    let balance = match account.balance.parse::<u64>() {
        Ok(b) => b,
        Err(_) => return AccountCheckStatus::InvalidBalanceFormat,
    };

    if account.status != "active" {
        AccountCheckStatus::Inactive
    } else if balance < min_balance {
        AccountCheckStatus::InsufficientBalance {
            balance,
            required: min_balance,
        }
    } else {
        AccountCheckStatus::Valid
    }
}

pub async fn check_accounts(
    client: &dyn RestClient,
    addresses: Vec<TonAddress>,
    min_balance: u64,
    forever: bool,
) {
    loop {
        match client.get_account_states(addresses.clone()).await {
            Ok(accounts) => {
                for account in accounts {
                    match check_account_status(&account, min_balance) {
                        AccountCheckStatus::Valid => {
                            info!(
                                "Account {} is active and has sufficient balance: {}",
                                account.address, account.balance
                            );
                        }
                        AccountCheckStatus::Inactive => {
                            error!(
                                "Account {} is not active. Status: {}",
                                account.address, account.status
                            );
                        }
                        AccountCheckStatus::InsufficientBalance { balance, required } => {
                            error!(
                                "Account {} has insufficient balance. Balance: {}, Required: {}",
                                account.address, balance, required
                            );
                        }
                        AccountCheckStatus::InvalidBalanceFormat => {
                            error!(
                                "Invalid balance for address {}: {}",
                                account.address, account.balance
                            );
                        }
                    }
                }
            }
            Err(err) => {
                error!("Failed to fetch account states: {:?}", err);
            }
        }

        if !forever {
            break;
        }
        sleep(Duration::from_secs(45)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockRestClient;
    use std::str::FromStr;
    use tracing_test::traced_test;

    fn make_account(address: TonAddress, balance: &str, status: &str) -> AccountState {
        AccountState {
            address,
            account_state_hash: "hash".to_string(),
            balance: balance.to_string(),
            status: status.to_string(),
        }
    }

    #[test]
    fn test_valid_account() {
        let acc = make_account(
            TonAddress::from_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            "1000",
            "active",
        );
        assert_eq!(check_account_status(&acc, 500), AccountCheckStatus::Valid);
    }

    #[test]
    fn test_inactive_account() {
        let acc = make_account(
            TonAddress::from_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            "1000",
            "inactive",
        );
        assert_eq!(
            check_account_status(&acc, 500),
            AccountCheckStatus::Inactive
        );
    }

    #[test]
    fn test_insufficient_balance() {
        let acc = make_account(
            TonAddress::from_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            "100",
            "active",
        );
        assert_eq!(
            check_account_status(&acc, 500),
            AccountCheckStatus::InsufficientBalance {
                balance: 100,
                required: 500
            }
        );
    }

    #[test]
    fn test_invalid_balance_format() {
        let acc = make_account(
            TonAddress::from_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            "notanumber",
            "active",
        );
        assert_eq!(
            check_account_status(&acc, 500),
            AccountCheckStatus::InvalidBalanceFormat
        );
    }

    #[tokio::test(start_paused = true)]
    #[traced_test]
    async fn test_check_accounts_integration_with_mock() {
        let mut mock = MockRestClient::new();

        let addresses = vec![
            TonAddress::from_str(
                "0:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap()
            .clone(),
            TonAddress::from_str(
                "0:00000000000000000000000000000000000000000000000000000000000000ff",
            )
            .unwrap()
            .clone(),
            TonAddress::from_str(
                "0:000000000000000000000000000000000000000000000000000000000000ffff",
            )
            .unwrap()
            .clone(),
        ];

        let expected_accounts = vec![
            make_account(addresses[0].clone(), "1000000", "active"), // OK
            make_account(addresses[1].clone(), "50", "active"),      // Insufficient balance
            make_account(addresses[2].clone(), "1000000", "inactive"), // Inactive
        ];

        let accounts_clone = expected_accounts.clone();

        mock.expect_get_account_states()
            .returning(move |_| Ok(accounts_clone.clone()));

        let handle = tokio::spawn(async move {
            check_accounts(&mock, addresses.clone(), 100, false).await;
        });

        handle.await.unwrap();

        assert!(logs_contain("Account EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c is active and has sufficient balance: 1000000"));
        assert!(logs_contain("Account EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA_9Gs has insufficient balance. Balance: 50, Required: 100"));
        assert!(logs_contain("Account EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD__9JT is not active. Status: inactive"));
    }
}
