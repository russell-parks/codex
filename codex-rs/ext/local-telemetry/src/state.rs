use std::sync::Arc;
use std::time::Instant;

use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::SessionSummary;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LocalTelemetryRunState {
    pub session_id: String,
    pub started_at: Instant,
    pub started_at_rfc3339: String,
    pub writer: Arc<dyn LocalTelemetryWriter>,
    pub summary: Arc<Mutex<SessionSummary>>,
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
pub struct SessionTelemetryBootstrap {
    pub invocation_mode: String,
    pub cwd: String,
    pub rollout_path: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub active_profile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionStopMetadata {
    pub rollout_path: Option<String>,
}

#[derive(Clone)]
pub struct LocalTelemetryWriterHandle {
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
