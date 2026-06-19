use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use chrono::DateTime;
use chrono::FixedOffset;
use chrono::NaiveDate;
use chrono::Utc;

use crate::DailyRollup;
use crate::SessionSummary;
use crate::TelemetryEvent;

#[derive(Debug, Clone)]
pub struct LocalTelemetryStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneResult {
    pub removed_summaries: u64,
    pub removed_event_files: u64,
    pub removed_rollups: u64,
}

impl LocalTelemetryStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    pub fn rollups_dir(&self) -> PathBuf {
        self.root.join("rollups")
    }

    pub fn list_summaries(&self) -> io::Result<Vec<SessionSummary>> {
        let mut summaries = Vec::new();
        for path in walk_files(self.runs_dir().as_path())? {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let payload = fs::read_to_string(path)?;
            let summary: SessionSummary =
                serde_json::from_str(&payload).map_err(io::Error::other)?;
            summaries.push(summary);
        }

        summaries.sort_by(|left, right| right.started_at.cmp(&left.started_at));
        Ok(summaries)
    }

    pub fn read_summary(&self, session_id: &str) -> io::Result<Option<SessionSummary>> {
        let path = self.runs_dir().join(format!("{session_id}.json"));
        if !path.exists() {
            return Ok(None);
        }

        let payload = fs::read_to_string(path)?;
        let summary: SessionSummary = serde_json::from_str(&payload).map_err(io::Error::other)?;
        Ok(Some(summary))
    }

    pub fn list_rollups(&self) -> io::Result<Vec<DailyRollup>> {
        let mut rollups = Vec::new();
        for path in walk_files(self.rollups_dir().as_path())? {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let payload = fs::read_to_string(path)?;
            let rollup: DailyRollup = serde_json::from_str(&payload).map_err(io::Error::other)?;
            rollups.push(rollup);
        }

        rollups.sort_by(|left, right| right.date.cmp(&left.date));
        Ok(rollups)
    }

    pub fn disk_usage_bytes(&self) -> io::Result<u64> {
        disk_usage_bytes(self.root.as_path())
    }

    pub fn latest_event_timestamp(&self) -> io::Result<Option<String>> {
        let mut latest: Option<DateTime<FixedOffset>> = None;

        for path in walk_files(self.events_dir().as_path())? {
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }

            let Some(event) = read_last_event(path.as_path())? else {
                continue;
            };
            let timestamp = parse_rfc3339(&event.timestamp)?;
            if latest.as_ref().is_none_or(|current| timestamp > *current) {
                latest = Some(timestamp);
            }
        }

        Ok(latest.map(|value| value.to_rfc3339()))
    }

    pub fn prune_older_than(&self, cutoff: DateTime<Utc>) -> io::Result<PruneResult> {
        let mut result = PruneResult::default();
        for summary in self.list_summaries()? {
            let timestamp = summary
                .ended_at
                .as_deref()
                .unwrap_or(summary.started_at.as_str());
            let parsed = parse_rfc3339(timestamp)?.with_timezone(&Utc);
            if parsed >= cutoff {
                continue;
            }

            let summary_path = self.runs_dir().join(format!("{}.json", summary.session_id));
            if summary_path.exists() {
                fs::remove_file(&summary_path)?;
                cleanup_empty_parents(summary_path.parent(), self.runs_dir().as_path())?;
                result.removed_summaries += 1;
            }

            let raw_event_path = PathBuf::from(&summary.raw_event_path);
            if raw_event_path.starts_with(self.root.as_path()) && raw_event_path.exists() {
                fs::remove_file(&raw_event_path)?;
                cleanup_empty_parents(raw_event_path.parent(), self.events_dir().as_path())?;
                result.removed_event_files += 1;
            }
        }

        let cutoff_date = cutoff.date_naive();
        for path in walk_files(self.rollups_dir().as_path())? {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                continue;
            };
            if date >= cutoff_date {
                continue;
            }

            fs::remove_file(&path)?;
            cleanup_empty_parents(path.parent(), self.rollups_dir().as_path())?;
            result.removed_rollups += 1;
        }

        Ok(result)
    }
}

fn walk_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn disk_usage_bytes(root: &Path) -> io::Result<u64> {
    Ok(walk_files(root)?
        .into_iter()
        .map(|path| fs::metadata(path).map(|value| value.len()))
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .sum())
}

fn read_last_event(path: &Path) -> io::Result<Option<TelemetryEvent>> {
    let payload = fs::read_to_string(path)?;
    let Some(line) = payload.lines().rev().find(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    let event = serde_json::from_str(line).map_err(io::Error::other)?;
    Ok(Some(event))
}

fn parse_rfc3339(value: &str) -> io::Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).map_err(io::Error::other)
}

fn cleanup_empty_parents(mut current: Option<&Path>, stop_at: &Path) -> io::Result<()> {
    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }
        if fs::read_dir(dir)?.next().is_some() {
            break;
        }
        fs::remove_dir(dir)?;
        current = dir.parent();
    }

    Ok(())
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
