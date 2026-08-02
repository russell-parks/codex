use serde::Deserialize;
use serde::Serialize;

pub const TELEMETRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelemetryEvent {
    pub schema_version: u32,
    pub timestamp: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub event_type: TelemetryEventType,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEventType {
    SessionStarted,
    TurnStarted,
    TurnCompleted,
    TurnAborted,
    TurnErrored,
    TurnItemRecorded,
    TokenUsageCheckpoint,
    TurnProfileRecorded,
    RateLimitsRecorded,
    ToolCallStarted,
    ToolCallFinished,
    ApprovalRecorded,
    SessionCompleted,
}
