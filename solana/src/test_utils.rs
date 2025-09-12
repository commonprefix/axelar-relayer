#[cfg(test)]
pub mod fixtures {
    use std::fs;

    use solana_types::solana_types::{RpcGetTransactionResponse, SolanaTransaction};

    pub fn transaction_fixtures() -> Vec<SolanaTransaction> {
        let file_path = "tests/testdata/transactions/solana_transaction.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        serde_json::from_str::<Vec<SolanaTransaction>>(&body)
            .expect("Failed to deserialize SolanaTransaction array")
    }

    pub fn rpc_response_fixtures() -> Vec<RpcGetTransactionResponse> {
        let file_path = "tests/testdata/rpc_batch_repsonse.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        serde_json::from_str::<Vec<RpcGetTransactionResponse>>(&body)
            .expect("Failed to deserialize RpcGetTransactionResponse array")
    }
}
