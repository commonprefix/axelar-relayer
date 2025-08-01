#[cfg(test)]
pub(crate) mod fixtures {
    use crate::gmp_api::gmp_types::{
        Amount, CannotExecuteMessageReason, CommonEventFields, CommonTaskFields,
        ConstructProofTask, ConstructProofTaskFields, Event, EventAttribute, EventMetadata,
        ExecuteTask, ExecuteTaskFields, GatewayTxTask, GatewayTxTaskFields, GatewayV2Message,
        MessageApprovedEventMetadata, MessageExecutedEventMetadata, MessageExecutionStatus,
        QuorumReachedEvent, ReactToExpiredSigningSessionTask,
        ReactToExpiredSigningSessionTaskFields, ReactToRetriablePollTask,
        ReactToRetriablePollTaskFields, ReactToWasmEventTask, ReactToWasmEventTaskFields,
        RefundTask, RefundTaskFields, Task, UnknownTask, VerificationStatus, VerifyTask,
        VerifyTaskFields, WasmEvent,
    };
    use serde_json::json;

    pub fn gas_refunded_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "GAS_REFUNDED".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(EventMetadata {
                tx_id: Some("tx123".to_string()),
                from_address: Some("from123".to_string()),
                finalized: Some(true),
                source_context: None,
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            }),
        };

        Event::GasRefunded {
            common,
            message_id,
            recipient_address: "recipient123".to_string(),
            refunded_amount: Amount {
                amount: "100".to_string(),
                token_id: Some("token".to_string()),
            },
            cost: Amount {
                amount: "10".to_string(),
                token_id: Some("token".to_string()),
            },
        }
    }

    pub fn gas_credit_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "GAS_CREDIT".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(EventMetadata {
                tx_id: Some("tx123".to_string()),
                from_address: Some("from123".to_string()),
                finalized: Some(true),
                source_context: None,
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            }),
        };

        Event::GasCredit {
            common,
            message_id,
            refund_address: "refund123".to_string(),
            payment: Amount {
                amount: "100".to_string(),
                token_id: Some("token".to_string()),
            },
        }
    }

    pub fn cannot_execute_message_v2_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "CANNOT_EXECUTE_MESSAGE_V2".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(EventMetadata {
                tx_id: Some("tx123".to_string()),
                from_address: Some("from123".to_string()),
                finalized: Some(true),
                source_context: None,
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            }),
        };

        Event::CannotExecuteMessageV2 {
            common,
            message_id,
            source_chain: "source123".to_string(),
            reason: CannotExecuteMessageReason::InsufficientGas,
            details: "details123".to_string(),
        }
    }

    pub fn its_interchain_transfer_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "ITS_INTERCHAIN_TRANSFER".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(EventMetadata {
                tx_id: Some("tx123".to_string()),
                from_address: Some("from123".to_string()),
                finalized: Some(true),
                source_context: None,
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            }),
        };

        Event::ITSInterchainTransfer {
            common,
            message_id,
            destination_chain: "destination123".to_string(),
            token_spent: Amount {
                amount: "100".to_string(),
                token_id: Some("token".to_string()),
            },
            source_address: "source123".to_string(),
            destination_address: "destination123".to_string(),
            data_hash: "hash123".to_string(),
        }
    }

    pub fn message_executed_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "MESSAGE_EXECUTED".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(MessageExecutedEventMetadata {
                common_meta: EventMetadata {
                    tx_id: Some("tx123".to_string()),
                    from_address: Some("from123".to_string()),
                    finalized: Some(true),
                    source_context: None,
                    timestamp: "2023-01-01T00:00:00Z".to_string(),
                },
                command_id: Some("command123".to_string()),
                child_message_ids: Some(vec!["child123".to_string()]),
                revert_reason: None,
            }),
        };

        Event::MessageExecuted {
            common,
            message_id,
            source_chain: "source123".to_string(),
            status: MessageExecutionStatus::SUCCESSFUL,
            cost: Amount {
                amount: "10".to_string(),
                token_id: Some("token".to_string()),
            },
        }
    }

    pub fn call_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "CALL".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(EventMetadata {
                tx_id: Some("tx123".to_string()),
                from_address: Some("from123".to_string()),
                finalized: Some(true),
                source_context: None,
                timestamp: "2023-01-01T00:00:00Z".to_string(),
            }),
        };

        let message = GatewayV2Message {
            message_id,
            source_chain: "source123".to_string(),
            source_address: "source_address123".to_string(),
            destination_address: "destination_address123".to_string(),
            payload_hash: "payload_hash123".to_string(),
        };

        Event::Call {
            common,
            message,
            destination_chain: "destination123".to_string(),
            payload: "payload123".to_string(),
        }
    }

    pub fn message_approved_event() -> Event {
        let event_id = "event123".to_string();
        let event_type = "MESSAGE_APPROVED".to_string();
        let message_id = "message123".to_string();

        let common = CommonEventFields {
            r#type: event_type,
            event_id,
            meta: Some(MessageApprovedEventMetadata {
                common_meta: EventMetadata {
                    tx_id: Some("tx123".to_string()),
                    from_address: Some("from123".to_string()),
                    finalized: Some(true),
                    source_context: None,
                    timestamp: "2023-01-01T00:00:00Z".to_string(),
                },
                command_id: Some("command123".to_string()),
            }),
        };

        let message = GatewayV2Message {
            message_id,
            source_chain: "source123".to_string(),
            source_address: "source_address123".to_string(),
            destination_address: "destination_address123".to_string(),
            payload_hash: "payload_hash123".to_string(),
        };

        Event::MessageApproved {
            common,
            message,
            cost: Amount {
                amount: "10".to_string(),
                token_id: Some("token".to_string()),
            },
        }
    }

    pub fn common_task_fields(task_type: &str) -> CommonTaskFields {
        CommonTaskFields {
            id: format!("{}_task_123", task_type.to_lowercase()),
            chain: "ton".to_string(),
            timestamp: "2023-01-01T00:00:00Z".to_string(),
            r#type: task_type.to_string(),
            meta: None,
        }
    }

    pub fn gateway_message() -> GatewayV2Message {
        GatewayV2Message {
            message_id: "message123".to_string(),
            source_chain: "ton".to_string(),
            source_address: "0x123".to_string(),
            destination_address: "0x456".to_string(),
            payload_hash: "0x789".to_string(),
        }
    }

    pub fn execute_task() -> Task {
        let common_fields = common_task_fields("EXECUTE");
        let message = gateway_message();

        let execute_task_fields = ExecuteTaskFields {
            message,
            payload: "0xabcdef".to_string(),
            available_gas_balance: Amount {
                token_id: None,
                amount: "1000000".to_string(),
            },
        };

        let execute_task = ExecuteTask {
            common: common_fields,
            task: execute_task_fields,
        };

        Task::Execute(execute_task)
    }

    pub fn verify_task() -> Task {
        let common_fields = common_task_fields("VERIFY");
        let message = gateway_message();

        let verify_task_fields = VerifyTaskFields {
            message,
            payload: "0xabcdef".to_string(),
        };

        let verify_task = VerifyTask {
            common: common_fields,
            task: verify_task_fields,
        };

        Task::Verify(verify_task)
    }

    pub fn construct_proof_task() -> Task {
        let common_fields = common_task_fields("CONSTRUCT_PROOF");
        let message = gateway_message();

        let construct_proof_task_fields = ConstructProofTaskFields {
            message,
            payload: "0xabcdef".to_string(),
        };

        let construct_proof_task = ConstructProofTask {
            common: common_fields,
            task: construct_proof_task_fields,
        };

        Task::ConstructProof(construct_proof_task)
    }

    pub fn refund_task() -> Task {
        let common_fields = common_task_fields("REFUND");
        let message = gateway_message();

        let refund_task_fields = RefundTaskFields {
            message,
            refund_recipient_address: "0x789".to_string(),
            remaining_gas_balance: Amount {
                token_id: None,
                amount: "500000".to_string(),
            },
        };

        let refund_task = RefundTask {
            common: common_fields,
            task: refund_task_fields,
        };

        Task::Refund(refund_task)
    }

    pub fn gateway_tx_task() -> Task {
        let common_fields = common_task_fields("GATEWAY_TX");

        let gateway_tx_task_fields = GatewayTxTaskFields {
            execute_data: "base64_encoded_data".to_string(),
        };

        let gateway_tx_task = GatewayTxTask {
            common: common_fields,
            task: gateway_tx_task_fields,
        };

        Task::GatewayTx(gateway_tx_task)
    }

    pub fn react_to_wasm_event_task() -> Task {
        let common_fields = common_task_fields("REACT_TO_WASM_EVENT");

        let event_attributes = vec![
            EventAttribute {
                key: "key1".to_string(),
                value: "value1".to_string(),
            },
            EventAttribute {
                key: "key2".to_string(),
                value: "value2".to_string(),
            },
        ];

        let wasm_event = WasmEvent {
            attributes: event_attributes,
            r#type: "wasm_event_type".to_string(),
        };

        let react_to_wasm_event_task_fields = ReactToWasmEventTaskFields {
            event: wasm_event,
            height: 12345,
        };

        let react_to_wasm_event_task = ReactToWasmEventTask {
            common: common_fields,
            task: react_to_wasm_event_task_fields,
        };

        Task::ReactToWasmEvent(react_to_wasm_event_task)
    }

    pub fn react_to_expired_signing_session_task() -> Task {
        let common_fields = common_task_fields("REACT_TO_EXPIRED_SIGNING_SESSION");

        let react_to_expired_signing_session_task_fields = ReactToExpiredSigningSessionTaskFields {
            session_id: 12345,
            broadcast_id: "broadcast123".to_string(),
            invoked_contract_address: "0xabc".to_string(),
            request_payload: "request_payload_data".to_string(),
        };

        let react_to_expired_signing_session_task = ReactToExpiredSigningSessionTask {
            common: common_fields,
            task: react_to_expired_signing_session_task_fields,
        };

        Task::ReactToExpiredSigningSession(react_to_expired_signing_session_task)
    }

    pub fn react_to_retriable_poll_task() -> Task {
        let common_fields = common_task_fields("REACT_TO_RETRIABLE_POLL");

        let quorum_reached_events = vec![QuorumReachedEvent {
            status: VerificationStatus::SucceededOnSourceChain,
            content: json!({"key": "value"}),
        }];

        let react_to_retriable_poll_task_fields = ReactToRetriablePollTaskFields {
            poll_id: 12345,
            broadcast_id: "broadcast123".to_string(),
            invoked_contract_address: "0xabc".to_string(),
            request_payload: "request_payload_data".to_string(),
            quorum_reached_events: Some(quorum_reached_events),
        };

        let react_to_retriable_poll_task = ReactToRetriablePollTask {
            common: common_fields,
            task: react_to_retriable_poll_task_fields,
        };

        Task::ReactToRetriablePoll(react_to_retriable_poll_task)
    }

    pub fn unknown_task() -> Task {
        let common_fields = common_task_fields("UNKNOWN");

        let unknown_task = UnknownTask {
            common: common_fields,
        };

        Task::Unknown(unknown_task)
    }

    pub fn execute_task_with_id(id: &str) -> Task {
        let mut common_fields = common_task_fields("EXECUTE");
        common_fields.id = id.to_string();

        let message = gateway_message();

        let execute_task_fields = ExecuteTaskFields {
            message,
            payload: "0xabcdef".to_string(),
            available_gas_balance: Amount {
                token_id: None,
                amount: "1000000".to_string(),
            },
        };

        let execute_task = ExecuteTask {
            common: common_fields,
            task: execute_task_fields,
        };

        Task::Execute(execute_task)
    }
}
