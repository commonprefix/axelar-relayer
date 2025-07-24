use core::fmt;
use std::collections::{BTreeMap, HashMap};

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

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskMetadata {
    #[serde(rename = "txID")]
    pub tx_id: Option<String>,
    #[serde(rename = "fromAddress")]
    pub from_address: Option<String>,
    pub finalized: Option<bool>,
    #[serde(rename = "sourceContext")]
    pub source_context: Option<BTreeMap<String, String>>,
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
pub struct MessageApprovedEventMetadata {
    #[serde(flatten)]
    pub common_meta: EventMetadata,
    #[serde(rename = "commandID")]
    pub command_id: Option<String>,
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
    MessageApproved {
        #[serde(flatten)]
        common: CommonEventFields<MessageApprovedEventMetadata>,
        message: GatewayV2Message,
        cost: Amount,
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
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_react_to_expired_signing_session_task() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/gmp_tasks/valid_tasks/ReactToExpiredSigningSessionTask.json");
        let json_content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let tasks: Vec<serde_json::Value> = serde_json::from_str(&json_content)
            .expect("Failed to parse ReactToExpiredSigningSessionTask.json");

        let task = &tasks[2]; // specific test, could be more abstract in the future
        let maybe_actual_task: Result<ReactToExpiredSigningSessionTask, serde_json::Error> =
            serde_json::from_value(task.clone());
        assert!(
            maybe_actual_task.is_ok(),
            "Failed to parse task: {:?}",
            maybe_actual_task.err()
        );
        let actual_task = maybe_actual_task.unwrap();
        let serialized_task = serde_json::to_value(&actual_task).unwrap();
        assert_eq!(serialized_task, *task);
    }

    #[test]
    fn test_react_to_retriable_poll_task() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/gmp_tasks/valid_tasks/ReactToRetriablePollTask.json");
        let json_content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let tasks: Vec<serde_json::Value> = serde_json::from_str(&json_content)
            .expect("Failed to parse ReactToRetriablePollTask.json");

        let task = &tasks[1]; // specific test, could be more abstract in the future
        let maybe_actual_task: Result<ReactToRetriablePollTask, serde_json::Error> =
            serde_json::from_value(task.clone());
        assert!(
            maybe_actual_task.is_ok(),
            "Failed to parse task: {:?}",
            maybe_actual_task.err()
        );
        let actual_task = maybe_actual_task.unwrap();
        let serialized_task = serde_json::to_value(&actual_task).unwrap();
        assert_eq!(serialized_task, *task);
    }
}
