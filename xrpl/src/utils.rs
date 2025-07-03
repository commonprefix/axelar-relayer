use relayer_base::gmp_api::gmp_types::RetryTask;

pub fn message_id_from_retry_task(task: RetryTask) -> Result<Option<String>, anyhow::Error> {
    match task {
        RetryTask::ReactToRetriablePoll(task) => {
            let message: xrpl_gateway::msg::ExecuteMsg =
                serde_json::from_str(&task.task.request_payload)?;
            if let xrpl_gateway::msg::ExecuteMsg::VerifyMessages(messages) = message {
                let tx_id = messages[0].tx_id().tx_hash_as_hex_no_prefix();
                Ok(Some(tx_id.to_string()))
            } else {
                Err(anyhow::anyhow!("Unknown payload: {:?}", message))
            }
        }
        RetryTask::ReactToExpiredSigningSession(task) => {
            let message: xrpl_multisig_prover::msg::ExecuteMsg =
                serde_json::from_str(&task.task.request_payload)?;
            if let xrpl_multisig_prover::msg::ExecuteMsg::ConstructProof { cc_id, .. } = message {
                Ok(Some(cc_id.to_string()))
            } else if let xrpl_multisig_prover::msg::ExecuteMsg::TicketCreate = message {
                Ok(None)
            } else {
                Err(anyhow::anyhow!("Unknown payload: {:?}", message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use relayer_base::gmp_api::{
        self,
        gmp_types::{ReactToExpiredSigningSessionTask, ReactToRetriablePollTask},
    };

    use crate::utils::message_id_from_retry_task;

    #[test]
    fn message_id_from_expired_signing_session_task() {
        let task_str = r#"{
            "id": "0197159e-a704-7cce-b89b-e7eba3e9d7d7",
            "chain": "xrpl",
            "timestamp": "2025-05-28T06:40:08.453075Z",
            "type": "REACT_TO_EXPIRED_SIGNING_SESSION",
            "meta": {
                "txID" : null,
                "fromAddress" : null,
                "finalized" : null,
                "sourceContext" : null,
                "scopedMessages": [
                    {
                        "messageID": "0xb8ecb910c92c4937c548b7b1fe63c512d8f68743d41bfb539ca181999736d597-98806061",
                        "sourceChain": "axelar"
                    }
                ]
            },
            "task": {
                "sessionID": 874302,
                "broadcastID": "01971594-4e05-7ef5-869f-716616729956",
                "invokedContractAddress": "axelar1k82qfzu3l6rvc7twlp9lpwsnav507czl6xyrk0xv287t4439ymvsl6n470",
                "requestPayload": "{\"construct_proof\":{\"cc_id\":{\"message_id\":\"0xb8ecb910c92c4937c548b7b1fe63c512d8f68743d41bfb539ca181999736d597-98806061\",\"source_chain\":\"axelar\"},\"payload\":\"0000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000087872706c2d65766d00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001800000000000000000000000000000000000000000000000000000000000000000ba5a21ca88ef6bba2bfff5088994f90e1077e2a1cc3dcc38bd261f00fce2824f00000000000000000000000000000000000000000000000000000000000000c00000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000081b32000000000000000000000000000000000000000000000000000000000000001600000000000000000000000000000000000000000000000000000000000000014d07a2ebf4caded3297753dd95c8fc08e971300cf00000000000000000000000000000000000000000000000000000000000000000000000000000000000000227245616279676243506d397744565731325557504d37344c63314d38546970594d580000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\"}}"
            }
        }"#;

        let task: ReactToExpiredSigningSessionTask = serde_json::from_str(task_str).unwrap();
        let message_id = message_id_from_retry_task(
            gmp_api::gmp_types::RetryTask::ReactToExpiredSigningSession(task.clone()),
        )
        .unwrap();
        assert_eq!(
            message_id,
            Some("axelar_0xb8ecb910c92c4937c548b7b1fe63c512d8f68743d41bfb539ca181999736d597-98806061".to_string())
        );
    }

    #[test]
    fn message_id_from_expired_ticket_create_task() {
        let task_str = r#"{
            "id": "0197159e-a704-7cce-b89b-e7eba3e9d7d7",
            "chain": "xrpl",
            "timestamp": "2025-05-28T06:40:08.453075Z",
            "type": "REACT_TO_EXPIRED_SIGNING_SESSION",
            "meta": null,
            "task": {
                "sessionID": 874302,
                "broadcastID": "01971594-4e05-7ef5-869f-716616729956",
                "invokedContractAddress": "axelar1k82qfzu3l6rvc7twlp9lpwsnav507czl6xyrk0xv287t4439ymvsl6n470",
                "requestPayload": "\"ticket_create\""
            }
        }"#;

        let task: ReactToExpiredSigningSessionTask = serde_json::from_str(task_str).unwrap();
        let message_id = message_id_from_retry_task(
            gmp_api::gmp_types::RetryTask::ReactToExpiredSigningSession(task.clone()),
        )
        .unwrap();
        assert_eq!(message_id, None);
    }

    #[test]
    fn message_id_from_retriable_poll_task() {
        let task_str = r#"{
  "id": "019715e8-5570-7f31-a6cf-90ad32e2b9b6",
  "chain": "xrpl",
  "timestamp": "2025-05-28T08:00:37.239699Z",
  "type": "REACT_TO_RETRIABLE_POLL",
  "meta": null,
  "task": {
    "pollID": 1742631,
    "broadcastID": "019715e5-c882-7c12-83b8-e771588c1353",
    "invokedContractAddress": "axelar1pnynr6wnmchutkv6490mdqqxkz54fnrtmq8krqhvglhsqhmu7wzsnc86sy",
    "requestPayload": "{\"verify_messages\":[{\"add_gas_message\":{\"tx_id\":\"5fa140ff4b90c83df9fdfdc81595bd134f41d929694eedb15cf7fd1c511e8025\",\"amount\":{\"drops\":169771},\"msg_id\":\"67f6ddc5421acc8f17e3f1942d2fbc718295894f2ea1647229e054c125a261e8\",\"source_address\":\"rBs8uSfoAePdbr8eZtq7FK2QnTgvHyWAee\"}}]}",
    "quorumReachedEvents": [
      {
        "status": "not_found_on_source_chain",
        "content": {
          "add_gas_message": {
            "amount": {
              "drops": 169771
            },
            "msg_id": "67f6ddc5421acc8f17e3f1942d2fbc718295894f2ea1647229e054c125a261e8",
            "source_address": "rBs8uSfoAePdbr8eZtq7FK2QnTgvHyWAee",
            "tx_id": "5fa140ff4b90c83df9fdfdc81595bd134f41d929694eedb15cf7fd1c511e8025"
          }
        }
      }
    ]
  }
}"#;

        let task: ReactToRetriablePollTask = serde_json::from_str(task_str).unwrap();
        let message_id = message_id_from_retry_task(
            gmp_api::gmp_types::RetryTask::ReactToRetriablePoll(task.clone()),
        )
        .unwrap();
        assert_eq!(
            message_id,
            Some("5fa140ff4b90c83df9fdfdc81595bd134f41d929694eedb15cf7fd1c511e8025".to_string())
        );
    }
}
