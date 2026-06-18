use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::PromptMetadataSummary;
use codex_local_telemetry::SessionSummary;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
pub(crate) struct LocalTelemetryRunState {
    pub session_id: String,
    pub started_at: Instant,
    pub started_at_rfc3339: String,
    pub writer: Arc<dyn LocalTelemetryWriter>,
    pub write_run_summary: bool,
    pub summary: Arc<AsyncMutex<SessionSummary>>,
}

impl std::fmt::Debug for LocalTelemetryRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTelemetryRunState")
            .field("session_id", &self.session_id)
            .field("started_at", &self.started_at)
            .field("started_at_rfc3339", &self.started_at_rfc3339)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionStopMetadata {
    pub rollout_path: Option<String>,
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
