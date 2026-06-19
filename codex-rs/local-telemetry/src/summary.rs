use std::collections::BTreeMap;

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
    pub session_source: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub active_profile: Option<String>,
    pub cwd: Option<String>,
    pub repo_root: Option<String>,
    pub git: Option<GitSummary>,
    pub config_snapshot: Option<ConfigSnapshotSummary>,
    pub prompt_metadata: PromptMetadataSummary,
    pub raw_event_path: String,
    pub rollout_path: Option<String>,
    pub usage_totals: UsageTotals,
    pub turn_counts: TurnCounts,
    pub tool_summary: ToolSummary,
    pub approval_summary: ApprovalSummary,
    pub error_summary: ErrorSummary,
    pub changed_files_summary: ChangedFilesSummary,
    pub resumed_from: Option<String>,
    pub forked_from: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigSnapshotSummary {
    pub config_sources: Vec<ConfigSourceSummary>,
    pub developer_instructions_loaded: bool,
    pub user_instructions_loaded: bool,
    pub user_instruction_source: Option<String>,
    pub project_instructions_loaded: bool,
    pub project_instruction_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigSourceSummary {
    pub kind: String,
    pub source: String,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitSummary {
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub commit_sha_before: Option<String>,
    pub commit_sha_after: Option<String>,
    pub dirty_before: Option<bool>,
    pub dirty_after: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptMetadataSummary {
    pub prompt_byte_length: u64,
    pub prompt_token_estimate: Option<u64>,
    pub prompt_sha256: Option<String>,
    pub prompt_text: Option<String>,
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
pub struct TurnCounts {
    pub started: u64,
    pub completed: u64,
    pub aborted: u64,
    pub errored: u64,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFilesSummary {
    pub paths: Vec<String>,
    pub counts_by_extension: BTreeMap<String, u64>,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
}
