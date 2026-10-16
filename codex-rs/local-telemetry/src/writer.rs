use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use chrono::NaiveDate;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use crate::paths::event_file_path;
use crate::paths::rollup_file_path;
use crate::paths::summary_file_path;
use crate::rollup::DailyRollup;
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
    write_daily_rollups: bool,
    append_lock: Arc<Semaphore>,
    summary_lock: Arc<Semaphore>,
}

impl JsonlTelemetryWriter {
    pub fn new(
        root: PathBuf,
        date: NaiveDate,
        session_id: String,
        write_daily_rollups: bool,
    ) -> Self {
        let raw_event_path = event_file_path(root.as_path(), date, &session_id);
        let summary_path = summary_file_path(root.as_path(), &session_id);
        Self {
            root,
            raw_event_path,
            summary_path,
            write_daily_rollups,
            append_lock: Arc::new(Semaphore::new(1)),
            summary_lock: Arc::new(Semaphore::new(1)),
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
            let _append_guard = self
                .append_lock
                .acquire()
                .await
                .map_err(std::io::Error::other)?;
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
            let _summary_guard = self
                .summary_lock
                .acquire()
                .await
                .map_err(std::io::Error::other)?;
            ensure_parent_dir(self.summary_path.as_path()).await?;

            let mut payload = serde_json::to_vec_pretty(summary).map_err(std::io::Error::other)?;
            payload.push(b'\n');
            tokio::fs::write(self.summary_path.as_path(), payload).await?;

            if self.write_daily_rollups {
                update_daily_rollup(self.root.as_path(), summary).await?;
            }

            Ok(())
        })
    }
}

async fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn update_daily_rollup(root: &Path, summary: &SessionSummary) -> std::io::Result<()> {
    let date = summary
        .started_at
        .split('T')
        .next()
        .ok_or_else(|| std::io::Error::other("summary started_at missing date component"))?;
    let rollup_path = rollup_file_path(root, date);
    ensure_parent_dir(rollup_path.as_path()).await?;

    let mut rollup = match tokio::fs::read_to_string(rollup_path.as_path()).await {
        Ok(payload) => serde_json::from_str(&payload).map_err(std::io::Error::other)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            DailyRollup::new(date.to_string())
        }
        Err(err) => return Err(err),
    };
    rollup.add_summary(summary);

    let mut payload = serde_json::to_vec_pretty(&rollup).map_err(std::io::Error::other)?;
    payload.push(b'\n');
    tokio::fs::write(rollup_path.as_path(), payload).await
}
