use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use chrono::TimeZone;
use chrono::Utc;
use pretty_assertions::assert_eq;

use crate::ChangedFilesSummary;
use crate::ConfigSnapshotSummary;
use crate::ConfigSourceSummary;
use crate::DailyRollup;
use crate::LocalTelemetryStore;
use crate::PromptMetadataSummary;
use crate::SessionSummary;
use crate::TELEMETRY_SCHEMA_VERSION;
use crate::TelemetryEvent;
use crate::TelemetryEventType;
use crate::TurnCounts;
use crate::UsageTotals;

static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn list_summaries_returns_newest_first() {
    let test_dir = TestDir::new();
    write_summary(
        test_dir.path(),
        sample_summary(
            test_dir.path(),
            "session-1",
            "2026-06-17T10:00:00Z",
            "2026-06-17T10:05:00Z",
        ),
    );
    write_summary(
        test_dir.path(),
        sample_summary(
            test_dir.path(),
            "session-2",
            "2026-06-17T11:00:00Z",
            "2026-06-17T11:05:00Z",
        ),
    );

    let store = LocalTelemetryStore::new(test_dir.path().to_path_buf());
    let summaries = store.list_summaries().expect("summaries should load");

    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["session-2", "session-1"]
    );
}

#[test]
fn latest_event_timestamp_reads_latest_jsonl_event() {
    let test_dir = TestDir::new();
    write_event_file(
        test_dir.path(),
        "session-1",
        &[
            sample_event("2026-06-17T10:00:00Z", "session-1"),
            sample_event("2026-06-17T10:05:00Z", "session-1"),
        ],
    );
    write_event_file(
        test_dir.path(),
        "session-2",
        &[sample_event("2026-06-17T11:00:00Z", "session-2")],
    );

    let store = LocalTelemetryStore::new(test_dir.path().to_path_buf());

    assert_eq!(
        store
            .latest_event_timestamp()
            .expect("latest event should load"),
        Some("2026-06-17T11:00:00+00:00".to_string())
    );
}

#[test]
fn prune_older_than_removes_old_summaries_and_event_files() {
    let test_dir = TestDir::new();
    let old_summary = sample_summary(
        test_dir.path(),
        "session-old",
        "2026-06-01T10:00:00Z",
        "2026-06-01T10:05:00Z",
    );
    let new_summary = sample_summary(
        test_dir.path(),
        "session-new",
        "2026-06-17T10:00:00Z",
        "2026-06-17T10:05:00Z",
    );
    write_summary(test_dir.path(), old_summary);
    write_summary(test_dir.path(), new_summary);
    write_event_file(
        test_dir.path(),
        "session-old",
        &[sample_event("2026-06-01T10:05:00Z", "session-old")],
    );
    write_event_file(
        test_dir.path(),
        "session-new",
        &[sample_event("2026-06-17T10:05:00Z", "session-new")],
    );
    write_rollup(test_dir.path(), DailyRollup::new("2026-06-01".to_string()));
    write_rollup(test_dir.path(), DailyRollup::new("2026-06-17".to_string()));

    let store = LocalTelemetryStore::new(test_dir.path().to_path_buf());
    let result = store
        .prune_older_than(Utc.with_ymd_and_hms(2026, 6, 10, 0, 0, 0).unwrap())
        .expect("prune should succeed");

    assert_eq!(result.removed_summaries, 1);
    assert_eq!(result.removed_event_files, 1);
    assert_eq!(result.removed_rollups, 1);
    assert!(!test_dir.path().join("runs/session-old.json").exists());
    assert!(!event_path(test_dir.path(), "session-old").exists());
    assert!(!rollup_path(test_dir.path(), "2026-06-01").exists());
    assert!(test_dir.path().join("runs/session-new.json").exists());
    assert!(event_path(test_dir.path(), "session-new").exists());
    assert!(rollup_path(test_dir.path(), "2026-06-17").exists());
}

#[test]
fn disk_usage_counts_summary_and_event_files() {
    let test_dir = TestDir::new();
    write_summary(
        test_dir.path(),
        sample_summary(
            test_dir.path(),
            "session-1",
            "2026-06-17T10:00:00Z",
            "2026-06-17T10:05:00Z",
        ),
    );
    write_event_file(
        test_dir.path(),
        "session-1",
        &[sample_event("2026-06-17T10:05:00Z", "session-1")],
    );

    let store = LocalTelemetryStore::new(test_dir.path().to_path_buf());

    assert!(store.disk_usage_bytes().expect("disk usage should load") > 0);
}

#[test]
fn list_rollups_returns_newest_first() {
    let test_dir = TestDir::new();
    write_rollup(test_dir.path(), DailyRollup::new("2026-06-17".to_string()));
    write_rollup(test_dir.path(), DailyRollup::new("2026-06-18".to_string()));

    let store = LocalTelemetryStore::new(test_dir.path().to_path_buf());
    let rollups = store.list_rollups().expect("rollups should load");

    assert_eq!(
        rollups
            .iter()
            .map(|rollup| rollup.date.as_str())
            .collect::<Vec<_>>(),
        vec!["2026-06-18", "2026-06-17"]
    );
}

fn write_summary(root: &Path, summary: SessionSummary) {
    let path = root
        .join("runs")
        .join(format!("{}.json", summary.session_id));
    fs::create_dir_all(path.parent().expect("summary parent should exist"))
        .expect("summary parent should be creatable");
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&summary).expect("summary should serialize")
        ),
    )
    .expect("summary should write");
}

fn write_event_file(root: &Path, session_id: &str, events: &[TelemetryEvent]) {
    let path = event_path(root, session_id);
    fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event parent should be creatable");
    let payload = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("event should serialize"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{payload}\n")).expect("event file should write");
}

fn write_rollup(root: &Path, rollup: DailyRollup) {
    let path = rollup_path(root, &rollup.date);
    fs::create_dir_all(path.parent().expect("rollup parent should exist"))
        .expect("rollup parent should be creatable");
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&rollup).expect("rollup should serialize")
        ),
    )
    .expect("rollup should write");
}

fn event_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("events")
        .join("2026")
        .join("06")
        .join("17")
        .join(format!("{session_id}.jsonl"))
}

fn rollup_path(root: &Path, date: &str) -> PathBuf {
    root.join("rollups").join(format!("{date}.json"))
}

fn sample_event(timestamp: &str, session_id: &str) -> TelemetryEvent {
    TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        timestamp: timestamp.to_string(),
        session_id: session_id.to_string(),
        turn_id: Some("turn-1".to_string()),
        event_type: TelemetryEventType::SessionCompleted,
        payload: serde_json::json!({
            "ok": true,
        }),
    }
}

fn sample_summary(
    root: &Path,
    session_id: &str,
    started_at: &str,
    ended_at: &str,
) -> SessionSummary {
    SessionSummary {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        session_id: session_id.to_string(),
        started_at: started_at.to_string(),
        ended_at: Some(ended_at.to_string()),
        duration_ms: Some(300_000),
        invocation_mode: "exec".to_string(),
        session_source: "exec".to_string(),
        model: Some("gpt-5".to_string()),
        reasoning_effort: Some("medium".to_string()),
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        active_profile: Some("safe".to_string()),
        cwd: Some("/workspace".to_string()),
        repo_root: None,
        git: None,
        config_snapshot: Some(ConfigSnapshotSummary {
            config_sources: vec![ConfigSourceSummary {
                kind: "user".to_string(),
                source: "user (/tmp/.codex/config.toml)".to_string(),
                profile: Some("safe".to_string()),
            }],
            developer_instructions_loaded: true,
            user_instructions_loaded: false,
            user_instruction_source: None,
            project_instructions_loaded: false,
            project_instruction_sources: Vec::new(),
        }),
        prompt_metadata: PromptMetadataSummary::default(),
        raw_event_path: event_path(root, session_id).display().to_string(),
        rollout_path: None,
        usage_totals: UsageTotals::default(),
        turn_counts: TurnCounts::default(),
        tool_summary: Default::default(),
        approval_summary: Default::default(),
        error_summary: Default::default(),
        changed_files_summary: ChangedFilesSummary::default(),
        resumed_from: None,
        forked_from: None,
    }
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let id = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codex-local-telemetry-reader-tests-{}-{id}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).expect("stale test dir should be removable");
        }
        fs::create_dir_all(&path).expect("test dir should be creatable");
        Self { path }
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.path.exists() {
            fs::remove_dir_all(&self.path).expect("test dir should clean up");
        }
    }
}
