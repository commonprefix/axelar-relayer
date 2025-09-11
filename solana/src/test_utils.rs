#[cfg(test)]
pub(crate) mod fixtures {
    use std::fs;

    use solana_types::solana_types::SolanaTransaction;

    pub fn fixtures() -> Vec<SolanaTransaction> {
        let file_path = "tests/testdata/transactions/solana_transaction.json";
        let body = fs::read_to_string(file_path).expect("Failed to read JSON test file");
        serde_json::from_str::<Vec<SolanaTransaction>>(&body)
            .expect("Failed to deserialize SolanaTransaction array")
    }
}
