use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn event_file_path_uses_partitioned_date_layout() {
    let path = event_file_path(
        Path::new("/tmp/telemetry"),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 17).unwrap(),
        "session-1",
    );
    assert_eq!(
        path,
        PathBuf::from("/tmp/telemetry/events/2026/06/17/session-1.jsonl")
    );
}

#[test]
fn summary_file_path_uses_runs_directory() {
    let path = summary_file_path(Path::new("/tmp/telemetry"), "session-1");
    assert_eq!(path, PathBuf::from("/tmp/telemetry/runs/session-1.json"));
}

#[test]
fn prompt_text_is_not_stored_unless_enabled() {
    assert_eq!(maybe_store_prompt(false, "secret prompt"), None);
    assert_eq!(
        maybe_store_prompt(true, "visible prompt"),
        Some("visible prompt".to_string())
    );
}

#[test]
fn prompt_hash_is_not_stored_unless_enabled() {
    assert_eq!(maybe_hash_prompt(false, "secret prompt"), None);
    assert_eq!(
        maybe_hash_prompt(true, "secret prompt"),
        Some("d6051e73b4e9a50e6a735ffba9494dd514acb71df325045501b0cbc8d206e20f".to_string())
    );
}

#[test]
fn jsonl_writer_uses_expected_event_and_summary_paths() {
    let writer = JsonlTelemetryWriter::new(
        PathBuf::from("/tmp/telemetry"),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 17).unwrap(),
        "session-1".to_string(),
    );

    assert_eq!(writer.root(), Path::new("/tmp/telemetry"));
    assert_eq!(
        writer.raw_event_path(),
        Path::new("/tmp/telemetry/events/2026/06/17/session-1.jsonl")
    );
    assert_eq!(
        writer.summary_path(),
        Path::new("/tmp/telemetry/runs/session-1.json")
    );
}

#[test]
fn noop_writer_methods_succeed_without_creating_files() {
    let test_dir = TestDir::new();
    let writer = NoopTelemetryWriter;
    let event = sample_event("2026-06-17T12:00:00Z", TelemetryEventType::SessionStarted);
    let summary = sample_summary(
        test_dir.path().join("events.jsonl").display().to_string(),
        Some("2026-06-17T12:05:00Z".to_string()),
    );

    run_async(async {
        writer
            .append_event(&event)
            .await
            .expect("noop append should succeed");
        writer
            .write_summary(&summary)
            .await
            .expect("noop summary write should succeed");
    });

    assert_eq!(read_dir_entries(test_dir.path()), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn jsonl_writer_append_event_writes_jsonl_records() {
    let test_dir = TestDir::new();
    let writer = JsonlTelemetryWriter::new(
        test_dir.path().to_path_buf(),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 17).unwrap(),
        "session-1".to_string(),
    );
    let first_event = sample_event("2026-06-17T12:00:00Z", TelemetryEventType::SessionStarted);
    let second_event = sample_event("2026-06-17T12:01:00Z", TelemetryEventType::TurnCompleted);

    writer
        .append_event(&first_event)
        .await
        .expect("event append should succeed");
    writer
        .append_event(&second_event)
        .await
        .expect("second event append should succeed");

    let raw_contents = std::fs::read_to_string(writer.raw_event_path())
        .expect("raw event file should be readable");
    let actual_events = raw_contents
        .lines()
        .map(|line| {
            serde_json::from_str::<TelemetryEvent>(line)
                .expect("each line should deserialize as a telemetry event")
        })
        .collect::<Vec<_>>();

    assert_eq!(actual_events, vec![first_event, second_event]);
    assert_eq!(raw_contents.matches('\n').count(), 2);
}

#[tokio::test]
async fn jsonl_writer_write_summary_persists_pretty_json() {
    let test_dir = TestDir::new();
    let writer = JsonlTelemetryWriter::new(
        test_dir.path().to_path_buf(),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 17).unwrap(),
        "session-1".to_string(),
    );
    let summary = sample_summary(
        writer.raw_event_path().display().to_string(),
        Some("2026-06-17T12:05:00Z".to_string()),
    );

    writer
        .write_summary(&summary)
        .await
        .expect("summary write should succeed");

    let raw_contents =
        std::fs::read_to_string(writer.summary_path()).expect("summary file should be readable");
    let actual_summary = serde_json::from_str::<SessionSummary>(&raw_contents)
        .expect("summary file should deserialize");

    assert_eq!(actual_summary, summary);
    assert_eq!(
        raw_contents,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&actual_summary).unwrap()
        )
    );
}

fn sample_event(timestamp: &str, event_type: TelemetryEventType) -> TelemetryEvent {
    TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        timestamp: timestamp.to_string(),
        session_id: "session-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        event_type,
        payload: json!({
            "status": "ok",
            "step": 1,
        }),
    }
}

fn sample_summary(raw_event_path: String, ended_at: Option<String>) -> SessionSummary {
    SessionSummary {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        session_id: "session-1".to_string(),
        started_at: "2026-06-17T12:00:00Z".to_string(),
        ended_at,
        duration_ms: Some(300_000),
        invocation_mode: "cli".to_string(),
        session_source: "interactive".to_string(),
        model: Some("gpt-5-codex".to_string()),
        reasoning_effort: Some("medium".to_string()),
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        active_profile: Some("safe".to_string()),
        cwd: Some("/workspace".to_string()),
        repo_root: Some("/workspace".to_string()),
        git: Some(GitSummary {
            remote: Some("origin".to_string()),
            branch: Some("main".to_string()),
            commit_sha_before: Some("abc123".to_string()),
            commit_sha_after: Some("def456".to_string()),
            dirty_before: Some(false),
            dirty_after: Some(true),
        }),
        config_snapshot: Some(ConfigSnapshotSummary {
            config_sources: vec![ConfigSourceSummary {
                kind: "user".to_string(),
                source: "user (/workspace/.codex/config.toml)".to_string(),
                profile: Some("safe".to_string()),
            }],
            developer_instructions_loaded: true,
            user_instructions_loaded: false,
            user_instruction_source: None,
            project_instructions_loaded: true,
            project_instruction_sources: vec!["/workspace/AGENTS.md".to_string()],
        }),
        prompt_metadata: PromptMetadataSummary {
            prompt_byte_length: 13,
            prompt_token_estimate: Some(3),
            prompt_sha256: Some(
                "d6051e73b4e9a50e6a735ffba9494dd514acb71df325045501b0cbc8d206e20f".to_string(),
            ),
            prompt_text: None,
        },
        raw_event_path,
        rollout_path: Some("/workspace/rollout.json".to_string()),
        usage_totals: UsageTotals {
            input_tokens: 10,
            output_tokens: 5,
            reasoning_tokens: 2,
            cached_input_tokens: 0,
            total_tokens: 17,
        },
        turn_counts: TurnCounts {
            started: 1,
            completed: 1,
            aborted: 0,
            errored: 0,
        },
        tool_summary: ToolSummary {
            total_calls: 2,
            success_count: 2,
            failure_count: 0,
        },
        approval_summary: ApprovalSummary {
            total_requests: 1,
            approved_count: 1,
            denied_count: 0,
        },
        error_summary: ErrorSummary {
            error_count: 0,
            last_error: None,
        },
        changed_files_summary: ChangedFilesSummary {
            paths: vec!["src/lib.rs".to_string()],
            counts_by_extension: BTreeMap::from([(String::from("rs"), 1)]),
            insertions: Some(10),
            deletions: Some(2),
        },
        resumed_from: None,
        forked_from: None,
    }
}

fn run_async<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should be created")
        .block_on(future)
}

fn read_dir_entries(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .expect("test directory should exist")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .collect()
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let unique_id = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codex-local-telemetry-tests-{timestamp}-{unique_id}"
        ));
        std::fs::create_dir_all(&path).expect("test directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
