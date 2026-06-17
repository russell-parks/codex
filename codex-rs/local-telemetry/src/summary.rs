use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub schema_version: u32,
    pub session_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub invocation_mode: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub cwd: Option<String>,
    pub repo_root: Option<String>,
    pub raw_event_path: String,
    pub rollout_path: Option<String>,
    pub usage_totals: UsageTotals,
    pub tool_summary: ToolSummary,
    pub approval_summary: ApprovalSummary,
    pub error_summary: ErrorSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_input_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolSummary {
    pub total_calls: u64,
    pub success_count: u64,
    pub failure_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalSummary {
    pub total_requests: u64,
    pub approved_count: u64,
    pub denied_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorSummary {
    pub error_count: u64,
    pub last_error: Option<String>,
}
