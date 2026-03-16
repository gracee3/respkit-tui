use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Serialize)]
pub struct RpcRequest<'a, T>
where
    T: Serialize,
{
    pub jsonrpc: &'static str,
    pub id: String,
    pub method: &'a str,
    pub params: T,
}

impl<'a, T> RpcRequest<'a, T>
where
    T: Serialize,
{
    pub fn new(id: String, method: &'a str, params: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcResponseEnvelope {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Value,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcErrorPayload>,
}

impl RpcResponseEnvelope {
    pub fn id_string(&self) -> String {
        match &self.id {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            Value::Null => String::from("null"),
            other => other.to_string(),
        }
    }

    pub fn into_result<T>(self) -> Result<T, RpcResponseError>
    where
        T: DeserializeOwned,
    {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(RpcResponseError::InvalidVersion(self.jsonrpc));
        }
        if let Some(error) = self.error {
            return Err(RpcResponseError::Service(error));
        }
        let result = self.result.ok_or(RpcResponseError::MissingResult)?;
        serde_json::from_value(result).map_err(RpcResponseError::Deserialize)
    }
}

#[derive(Debug, Clone, Deserialize, Error)]
#[error("rpc error {code}: {message}")]
pub struct RpcErrorPayload {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Error)]
pub enum RpcResponseError {
    #[error("unexpected jsonrpc version: {0}")]
    InvalidVersion(String),
    #[error("missing result payload")]
    MissingResult,
    #[error(transparent)]
    Service(#[from] RpcErrorPayload),
    #[error("failed to deserialize rpc result: {0}")]
    Deserialize(serde_json::Error),
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LedgerInfo {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub ledger_path: String,
    #[serde(default)]
    pub row_count: usize,
    #[serde(default)]
    pub task_count: usize,
    #[serde(default)]
    pub service_version: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HealthStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub ledger_path: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LedgerTasks {
    #[serde(default)]
    pub task_names: Vec<String>,
    #[serde(default)]
    pub rows_by_task: std::collections::BTreeMap<String, usize>,
    #[serde(default)]
    pub registered_adapters: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskSummary {
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub approved: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub needs_review: usize,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LedgerSummary {
    #[serde(default)]
    pub counts: std::collections::BTreeMap<String, usize>,
    #[serde(default)]
    pub by_task: std::collections::BTreeMap<String, TaskSummary>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RowsListResult {
    #[serde(default)]
    pub rows: Vec<RowView>,
    #[serde(default)]
    pub count: usize,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RowResult {
    pub row: RowView,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HistoryResult {
    #[serde(default)]
    pub events: Vec<RowHistoryEvent>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PreviewResult {
    #[serde(default)]
    pub preview: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ValidationResult {
    #[serde(default)]
    pub valid: bool,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub approved_output: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeriveResult {
    #[serde(default)]
    pub approved_output: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DecisionResult {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub task_name: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub decision_source: Option<String>,
    #[serde(default)]
    pub decision_actor: Option<String>,
    #[serde(default)]
    pub decision_note: Option<String>,
    #[serde(default)]
    pub validation: Option<ValidationResult>,
    #[serde(default)]
    pub decision_payload: Option<Value>,
    #[serde(default)]
    pub approved_output: Option<Value>,
    #[serde(default)]
    pub row: Option<RowView>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionsListResult {
    #[serde(default)]
    pub actions: Vec<ActionDescriptor>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionDescriptor {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub requires_edits: bool,
    #[serde(default)]
    pub builtin: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionsInvokeResult {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub results: Vec<ActionOutcome>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionOutcome {
    #[serde(default)]
    pub task_name: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExportResult {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ShutdownResult {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RowView {
    #[serde(default)]
    pub task_name: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub item_locator: Option<String>,
    #[serde(default)]
    pub machine_status: String,
    #[serde(default)]
    pub human_status: String,
    #[serde(default)]
    pub rerun_eligible: bool,
    #[serde(default)]
    pub proposal_payload: Option<Value>,
    #[serde(default)]
    pub review_payload: Option<Value>,
    #[serde(default)]
    pub apply_payload: Option<Value>,
    #[serde(default)]
    pub human_decision_payload: Option<Value>,
    #[serde(default)]
    pub extras: Value,
    #[serde(default)]
    pub risk_flags: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub human_notes: Option<String>,
    #[serde(default)]
    pub decision_source: Option<String>,
    #[serde(default)]
    pub decision_actor: Option<String>,
    #[serde(default)]
    pub decision_note: Option<String>,
    #[serde(default)]
    pub decision_metadata: Value,
    #[serde(default)]
    pub rendered_summary: String,
    #[serde(default)]
    pub approved_output: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RowHistoryEvent {
    #[serde(default)]
    pub version: usize,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub machine_status: String,
    #[serde(default)]
    pub human_status: String,
    #[serde(default)]
    pub event_at: String,
    #[serde(default)]
    pub payload: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_success_payloads() {
        let envelope: RpcResponseEnvelope =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":"7","result":{"status":"ok"}}"#)
                .expect("response envelope should parse");
        let payload: ShutdownResult = envelope
            .into_result()
            .expect("success result should decode");
        assert_eq!(payload.status, "ok");
    }

    #[test]
    fn surfaces_service_errors() {
        let envelope: RpcResponseEnvelope = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":"8","error":{"code":-32602,"message":"bad params"}}"#,
        )
        .expect("response envelope should parse");
        let err = envelope
            .into_result::<ShutdownResult>()
            .expect_err("error payload should surface");
        match err {
            RpcResponseError::Service(payload) => assert_eq!(payload.code, -32602),
            other => panic!("unexpected error: {other}"),
        }
    }
}
