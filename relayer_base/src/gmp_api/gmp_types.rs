use core::fmt;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GatewayV2Message {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "sourceChain")]
    pub source_chain: String,
    #[serde(rename = "sourceAddress")]
    pub source_address: String,
    #[serde(rename = "destinationAddress")]
    pub destination_address: String,
    #[serde(rename = "payloadHash")]
    pub payload_hash: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Amount {
    #[serde(rename = "tokenID")]
    pub token_id: Option<String>,
    pub amount: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CommonTaskFields {
    pub id: String,
    pub chain: String,
    pub timestamp: String,
    pub r#type: String,
    pub meta: Option<TaskMetadata>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ExecuteTaskFields {
    pub message: GatewayV2Message,
    pub payload: String,
    #[serde(rename = "availableGasBalance")]
    pub available_gas_balance: Amount,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ExecuteTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: ExecuteTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GatewayTxTaskFields {
    #[serde(rename = "executeData")]
    pub execute_data: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GatewayTxTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: GatewayTxTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct VerifyTaskFields {
    pub message: GatewayV2Message,
    pub payload: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct VerifyTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: VerifyTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ConstructProofTaskFields {
    pub message: GatewayV2Message,
    pub payload: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ConstructProofTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: ConstructProofTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToWasmEventTaskFields {
    pub event: WasmEvent,
    pub height: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToExpiredSigningSessionTaskFields {
    #[serde(rename = "sessionID")]
    pub session_id: u64,
    #[serde(rename = "broadcastID")]
    pub broadcast_id: String,
    #[serde(rename = "invokedContractAddress")]
    pub invoked_contract_address: String,
    #[serde(rename = "requestPayload")]
    pub request_payload: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToExpiredSigningSessionTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: ReactToExpiredSigningSessionTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToRetriablePollTaskFields {
    #[serde(rename = "pollID")]
    pub poll_id: u64,
    #[serde(rename = "broadcastID")]
    pub broadcast_id: String,
    #[serde(rename = "invokedContractAddress")]
    pub invoked_contract_address: String,
    #[serde(rename = "requestPayload")]
    pub request_payload: String,
    #[serde(rename = "quorumReachedEvents")]
    pub quorum_reached_events: Option<Vec<QuorumReachedEvent>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct QuorumReachedEvent {
    pub status: VerificationStatus,
    pub content: Value,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToRetriablePollTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: ReactToRetriablePollTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum RetryTask {
    ReactToRetriablePoll(ReactToRetriablePollTask),
    ReactToExpiredSigningSession(ReactToExpiredSigningSessionTask),
}

impl RetryTask {
    pub fn request_payload(&self) -> String {
        match self {
            RetryTask::ReactToRetriablePoll(t) => t.task.request_payload.clone(),
            RetryTask::ReactToExpiredSigningSession(t) => t.task.request_payload.clone(),
        }
    }

    pub fn invoked_contract_address(&self) -> String {
        match self {
            RetryTask::ReactToRetriablePoll(t) => t.task.invoked_contract_address.clone(),
            RetryTask::ReactToExpiredSigningSession(t) => t.task.invoked_contract_address.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WasmEvent {
    pub attributes: Vec<EventAttribute>,
    pub r#type: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReactToWasmEventTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: ReactToWasmEventTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RefundTaskFields {
    pub message: GatewayV2Message,
    #[serde(rename = "refundRecipientAddress")]
    pub refund_recipient_address: String,
    #[serde(rename = "remainingGasBalance")]
    pub remaining_gas_balance: Amount,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RefundTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
    pub task: RefundTaskFields,
}

impl fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", serde_json::to_string(self).unwrap())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct UnknownTask {
    #[serde(flatten)]
    pub common: CommonTaskFields,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum Task {
    Verify(VerifyTask),
    Execute(ExecuteTask),
    GatewayTx(GatewayTxTask),
    ConstructProof(ConstructProofTask),
    ReactToWasmEvent(ReactToWasmEventTask),
    Refund(RefundTask),
    ReactToExpiredSigningSession(ReactToExpiredSigningSessionTask),
    ReactToRetriablePoll(ReactToRetriablePollTask),
    Unknown(UnknownTask),
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum TaskKind {
    Verify,
    Execute,
    GatewayTx,
    ConstructProof,
    ReactToWasmEvent,
    Refund,
    ReactToExpiredSigningSession,
    ReactToRetriablePoll,
    Unknown,
}

impl Task {
    pub fn id(&self) -> String {
        match self {
            Task::Execute(t) => t.common.id.clone(),
            Task::Verify(t) => t.common.id.clone(),
            Task::GatewayTx(t) => t.common.id.clone(),
            Task::ConstructProof(t) => t.common.id.clone(),
            Task::ReactToWasmEvent(t) => t.common.id.clone(),
            Task::Refund(t) => t.common.id.clone(),
            Task::ReactToExpiredSigningSession(t) => t.common.id.clone(),
            Task::ReactToRetriablePoll(t) => t.common.id.clone(),
            Task::Unknown(t) => t.common.id.clone(),
        }
    }

    pub fn kind(&self) -> TaskKind {
        use Task::*;
        match self {
            Verify(_) => TaskKind::Verify,
            Execute(_) => TaskKind::Execute,
            GatewayTx(_) => TaskKind::GatewayTx,
            ConstructProof(_) => TaskKind::ConstructProof,
            ReactToWasmEvent(_) => TaskKind::ReactToWasmEvent,
            Refund(_) => TaskKind::Refund,
            ReactToExpiredSigningSession(_) => TaskKind::ReactToExpiredSigningSession,
            ReactToRetriablePoll(_) => TaskKind::ReactToRetriablePoll,
            Unknown(_) => TaskKind::Unknown,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommonEventFields<T> {
    pub r#type: String,
    #[serde(rename = "eventID")]
    pub event_id: String,
    pub meta: Option<T>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskMetadata {
    #[serde(rename = "txID")]
    pub tx_id: Option<String>,
    #[serde(rename = "fromAddress")]
    pub from_address: Option<String>,
    pub finalized: Option<bool>,
    #[serde(rename = "sourceContext")]
    pub source_context: Option<HashMap<String, String>>,
    #[serde(rename = "scopedMessages")]
    pub scoped_messages: Option<Vec<ScopedMessage>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EventMetadata {
    #[serde(rename = "txID")]
    pub tx_id: Option<String>,
    #[serde(rename = "fromAddress")]
    pub from_address: Option<String>,
    pub finalized: Option<bool>,
    #[serde(rename = "sourceContext")]
    pub source_context: Option<HashMap<String, String>>,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageExecutedEventMetadata {
    #[serde(flatten)]
    pub common_meta: EventMetadata,
    #[serde(rename = "commandID")]
    pub command_id: Option<String>,
    #[serde(rename = "childMessageIDs")]
    pub child_message_ids: Option<Vec<String>>,
    #[serde(rename = "revertReason")]
    pub revert_reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ScopedMessage {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "sourceChain")]
    pub source_chain: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CannotExecuteMessageReason {
    InsufficientGas,
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum MessageExecutionStatus {
    SUCCESSFUL,
    REVERTED,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum VerificationStatus {
    #[serde(rename = "succeeded_on_source_chain")]
    SucceededOnSourceChain,
    #[serde(rename = "failed_on_source_chain")]
    FailedOnSourceChain,
    #[serde(rename = "failed_on_destination_chain")]
    FailedOnDestinationChain,
    #[serde(rename = "not_found_on_source_chain")]
    NotFoundOnSourceChain,
    #[serde(rename = "failed_to_verify")]
    FailedToVerify,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Event {
    Call {
        #[serde(flatten)]
        common: CommonEventFields<EventMetadata>,
        message: GatewayV2Message,
        #[serde(rename = "destinationChain")]
        destination_chain: String,
        payload: String,
    },
    GasRefunded {
        #[serde(flatten)]
        common: CommonEventFields<EventMetadata>,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "recipientAddress")]
        recipient_address: String,
        #[serde(rename = "refundedAmount")]
        refunded_amount: Amount,
        cost: Amount,
    },
    GasCredit {
        #[serde(flatten)]
        common: CommonEventFields<EventMetadata>,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "refundAddress")]
        refund_address: String,
        payment: Amount,
    },
    MessageExecuted {
        #[serde(flatten)]
        common: CommonEventFields<MessageExecutedEventMetadata>,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "sourceChain")]
        source_chain: String,
        status: MessageExecutionStatus,
        cost: Amount,
    },
    CannotExecuteMessageV2 {
        #[serde(flatten)]
        common: CommonEventFields<EventMetadata>,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "sourceChain")]
        source_chain: String,
        reason: CannotExecuteMessageReason,
        details: String,
    },
    ITSInterchainTransfer {
        #[serde(flatten)]
        common: CommonEventFields<EventMetadata>,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "destinationChain")]
        destination_chain: String,
        #[serde(rename = "tokenSpent")]
        token_spent: Amount,
        #[serde(rename = "sourceAddress")]
        source_address: String,
        #[serde(rename = "destinationAddress")]
        destination_address: String,
        #[serde(rename = "dataHash")]
        data_hash: String,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PostEventResult {
    pub status: String,
    pub index: usize,
    pub error: Option<String>,
    pub retriable: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PostEventResponse {
    pub results: Vec<PostEventResult>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct EventMessage {
    pub events: Vec<Event>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BroadcastRequest {
    Generic(Value),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum QueryRequest {
    Generic(Value),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StorePayloadResult {
    pub keccak256: String,
}

#[cfg(test)]
mod tests {
    use super::{ReactToExpiredSigningSessionTask, ReactToRetriablePollTask};

    #[test]
    fn test_react_to_expired_signing_session_task() {
        let task = r#"{
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

        let maybe_actual_task: Result<ReactToExpiredSigningSessionTask, serde_json::Error> =
            serde_json::from_str(task);
        assert!(
            maybe_actual_task.is_ok(),
            "Failed to parse task: {:?}",
            maybe_actual_task.err()
        );
        let actual_task = maybe_actual_task.unwrap();
        let maybe_serialized_task = serde_json::to_string(&actual_task);
        assert!(maybe_serialized_task.is_ok());
        let actual_serialized_task = maybe_serialized_task.unwrap();

        assert_eq!(
            actual_serialized_task,
            task.split_whitespace().collect::<String>()
        );
    }

    #[test]
    fn test_react_to_retriable_poll_task() {
        let task = r#"{
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

        let maybe_actual_task: Result<ReactToRetriablePollTask, serde_json::Error> =
            serde_json::from_str(task);
        assert!(
            maybe_actual_task.is_ok(),
            "Failed to parse task: {:?}",
            maybe_actual_task.err()
        );
        let actual_task = maybe_actual_task.unwrap();
        let maybe_serialized_task = serde_json::to_string(&actual_task);
        assert!(maybe_serialized_task.is_ok());
        let actual_serialized_task = maybe_serialized_task.unwrap();

        assert_eq!(
            actual_serialized_task,
            task.split_whitespace().collect::<String>()
        );
    }
}
