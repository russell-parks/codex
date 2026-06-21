use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::GitSummary;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::PromptMetadataSummary;
use codex_local_telemetry::RuntimeSummary;
use codex_local_telemetry::SessionSummary;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
pub(crate) struct LocalTelemetryRunState {
    pub session_id: String,
    pub started_at: Instant,
    pub started_at_rfc3339: String,
    pub writer: Arc<dyn LocalTelemetryWriter>,
    pub write_run_summary: bool,
    pub capture_session: bool,
    pub capture_turns: bool,
    pub capture_usage: bool,
    pub capture_tool_calls: bool,
    pub capture_approvals: bool,
    pub capture_errors: bool,
    pub log_assistant_text: bool,
    pub log_tool_output: bool,
    pub log_diffs: bool,
    pub summary: Arc<AsyncMutex<SessionSummary>>,
}

impl std::fmt::Debug for LocalTelemetryRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTelemetryRunState")
            .field("session_id", &self.session_id)
            .field("started_at", &self.started_at)
            .field("started_at_rfc3339", &self.started_at_rfc3339)
            .field("capture_session", &self.capture_session)
            .field("capture_turns", &self.capture_turns)
            .field("capture_usage", &self.capture_usage)
            .field("capture_tool_calls", &self.capture_tool_calls)
            .field("capture_approvals", &self.capture_approvals)
            .field("capture_errors", &self.capture_errors)
            .field("log_assistant_text", &self.log_assistant_text)
            .field("log_tool_output", &self.log_tool_output)
            .field("log_diffs", &self.log_diffs)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct SessionStopUpdate {
    pub rollout_path: Option<String>,
    pub git: Option<GitSummary>,
    pub changed_files_summary: Option<ChangedFilesSummary>,
    pub runtime_summary: Option<RuntimeSummary>,
    pub final_outcome: Option<String>,
    pub abort_reason: Option<String>,
}

#[derive(Clone)]
pub(crate) struct LocalTelemetryWriterHandle {
    pub raw_event_path: String,
    pub writer: Arc<dyn LocalTelemetryWriter>,
}

impl std::fmt::Debug for LocalTelemetryWriterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTelemetryWriterHandle")
            .field("raw_event_path", &self.raw_event_path)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(crate) struct ToolCallTimingState {
    by_call_id: Mutex<HashMap<String, ToolCallTiming>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolCallTiming {
    pub started_at: Instant,
}

impl ToolCallTimingState {
    pub(crate) fn insert(&self, call_id: String, timing: ToolCallTiming) {
        self.by_call_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(call_id, timing);
    }

    pub(crate) fn remove(&self, call_id: &str) -> Option<ToolCallTiming> {
        self.by_call_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(call_id)
    }
}

#[derive(Debug, Default)]
pub(crate) struct PromptCaptureState {
    by_turn_id: Mutex<HashMap<String, PromptMetadataSummary>>,
}

impl PromptCaptureState {
    pub(crate) fn insert(&self, turn_id: String, metadata: PromptMetadataSummary) {
        self.by_turn_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(turn_id, metadata);
    }

    pub(crate) fn remove(&self, turn_id: &str) -> Option<PromptMetadataSummary> {
        self.by_turn_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(turn_id)
    }
}
