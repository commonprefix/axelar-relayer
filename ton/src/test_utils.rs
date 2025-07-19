#[cfg(test)]
pub(crate) mod fixtures {
    use std::fs;
    use relayer_base::ton_types::{Trace, TracesResponse, TracesResponseRest};

    pub fn fixture_traces() -> Vec<Trace> {
        let file_path = "tests/data/v3_traces.json";
        let body = fs::read(file_path).expect("Failed to read JSON test file");
        let clean_text = crate::client::clean_json_string_full(&body);

        let rest: TracesResponseRest =
            serde_json::from_str(&clean_text).expect("Failed to deserialize test transaction data");

        TracesResponse::from(rest).traces
    }

}