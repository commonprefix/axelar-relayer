#[cfg(test)]
pub mod fixtures {
    use std::fs;

    use solana_transaction_status_client_types::EncodedConfirmedTransactionWithStatusMeta;

    use crate::solana_types::RpcGetTransactionResponse;

    pub fn rpc_response_fixtures() -> Vec<RpcGetTransactionResponse> {
        let file_path = "../solana/tests/testdata/rpc_batch_repsonse.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        serde_json::from_str::<Vec<RpcGetTransactionResponse>>(&body)
            .expect("Failed to deserialize RpcGetTransactionResponse array")
    }

    pub fn encoded_confirmed_tx_with_meta_fixtures(
    ) -> Vec<EncodedConfirmedTransactionWithStatusMeta> {
        let file_path = "../solana/tests/testdata/transactions/encoded_confirmed_tx_with_meta.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        serde_json::from_str::<Vec<EncodedConfirmedTransactionWithStatusMeta>>(&body)
            .expect("Failed to deserialize EncodedConfirmedTransactionWithStatusMeta array")
    }
}
