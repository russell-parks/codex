use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

use chrono::NaiveDate;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use crate::paths::event_file_path;
use crate::paths::summary_file_path;
use crate::schema::TelemetryEvent;
use crate::summary::SessionSummary;

type LocalTelemetryFuture<'a> = Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>>;

/// Persists raw local telemetry events and session summaries to a storage sink.
///
/// Implementations are intentionally small: append JSONL events, write a single
/// JSON summary, or no-op when local telemetry is disabled.
pub trait LocalTelemetryWriter: Send + Sync {
    fn append_event<'a>(&'a self, event: &'a TelemetryEvent) -> LocalTelemetryFuture<'a>;

    fn write_summary<'a>(&'a self, summary: &'a SessionSummary) -> LocalTelemetryFuture<'a>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTelemetryWriter;

impl LocalTelemetryWriter for NoopTelemetryWriter {
    fn append_event<'a>(&'a self, _event: &'a TelemetryEvent) -> LocalTelemetryFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn write_summary<'a>(&'a self, _summary: &'a SessionSummary) -> LocalTelemetryFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, Clone)]
pub struct JsonlTelemetryWriter {
    root: PathBuf,
    raw_event_path: PathBuf,
    summary_path: PathBuf,
}

impl JsonlTelemetryWriter {
    pub fn new(root: PathBuf, date: NaiveDate, session_id: String) -> Self {
        let raw_event_path = event_file_path(root.as_path(), date, &session_id);
        let summary_path = summary_file_path(root.as_path(), &session_id);
        Self {
            root,
            raw_event_path,
            summary_path,
        }
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn raw_event_path(&self) -> &Path {
        self.raw_event_path.as_path()
    }

    pub fn summary_path(&self) -> &Path {
        self.summary_path.as_path()
    }
}

impl LocalTelemetryWriter for JsonlTelemetryWriter {
    fn append_event<'a>(&'a self, event: &'a TelemetryEvent) -> LocalTelemetryFuture<'a> {
        Box::pin(async move {
            ensure_parent_dir(self.raw_event_path.as_path()).await?;

            let mut payload = serde_json::to_vec(event).map_err(std::io::Error::other)?;
            payload.push(b'\n');

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.raw_event_path.as_path())
                .await?;
            file.write_all(&payload).await?;
            file.flush().await
        })
    }

    fn write_summary<'a>(&'a self, summary: &'a SessionSummary) -> LocalTelemetryFuture<'a> {
        Box::pin(async move {
            ensure_parent_dir(self.summary_path.as_path()).await?;

            let mut payload = serde_json::to_vec_pretty(summary).map_err(std::io::Error::other)?;
            payload.push(b'\n');
            tokio::fs::write(self.summary_path.as_path(), payload).await
        })
    }
}

async fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}
