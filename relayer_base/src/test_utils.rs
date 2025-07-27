#[cfg(test)]

pub(crate) mod fixtures {
    use crate::gmp_api::gmp_types::{
        Amount, CannotExecuteMessageReason, CommonEventFields, Event, EventMetadata,
        GatewayV2Message, MessageApprovedEventMetadata, MessageExecutedEventMetadata,
        MessageExecutionStatus,
    };

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
}
