use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use xrpl_amplifier_types::msg::XRPLMessage;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RouterMessage {
    // TODO: can this be imported?
    pub cc_id: String,
    pub source_address: String,
    pub destination_chain: String,
    pub destination_address: String,
    pub payload_hash: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GatewayV2Message {
    // TODO: can this be imported?
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
    pub meta: Option<Metadata>,
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum Task {
    Verify(VerifyTask),
    Execute(ExecuteTask),
    GatewayTx(GatewayTxTask),
    ConstructProof(ConstructProofTask),
    ReactToWasmEvent(ReactToWasmEventTask),
    Refund(RefundTask),
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
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommonEventFields {
    pub r#type: String,
    #[serde(rename = "eventID")]
    pub event_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Metadata {
    #[serde(rename = "txID")]
    pub tx_id: Option<String>,
    #[serde(rename = "fromAddress")]
    pub from_address: Option<String>,
    pub finalized: Option<bool>,
    #[serde(rename = "sourceContext")]
    pub source_context: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Event {
    Call {
        #[serde(flatten)]
        common: CommonEventFields,
        message: GatewayV2Message,
        #[serde(rename = "destinationChain")]
        destination_chain: String,
        payload: String,
        meta: Option<Metadata>,
    },
    GasRefunded {
        #[serde(flatten)]
        common: CommonEventFields,
        #[serde(rename = "recipientAddress")]
        recipient_address: String,
        #[serde(rename = "refundedAmount")]
        refunded_amount: Amount,
        cost: Amount,
    },
    GasCredit {
        #[serde(flatten)]
        common: CommonEventFields,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "refundAddress")]
        refund_address: String,
        payment: Amount,
        meta: Option<Metadata>,
    },
    MessageExecuted {
        #[serde(flatten)]
        common: CommonEventFields,
        #[serde(rename = "messageID")]
        message_id: String,
        #[serde(rename = "sourceChain")]
        source_chain: String,
        status: String,
        cost: Amount,
        meta: Option<Metadata>,
    },
    CannotExecuteMessage {
        #[serde(flatten)]
        common: CommonEventFields,
        #[serde(rename = "taskItemID")]
        task_item_id: String,
        reason: String,
        details: Amount,
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
