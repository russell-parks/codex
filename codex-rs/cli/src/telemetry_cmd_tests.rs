use std::path::PathBuf;
use std::time::Duration;

use pretty_assertions::assert_eq;

use crate::telemetry_cmd::GroupBy;
use crate::telemetry_cmd::build_report_rows;
use crate::telemetry_cmd::csv_field;
use crate::telemetry_cmd::parse_duration;
use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::PromptMetadataSummary;
use codex_local_telemetry::SessionSummary;
use codex_local_telemetry::TELEMETRY_SCHEMA_VERSION;
use codex_local_telemetry::TurnCounts;
use codex_local_telemetry::UsageTotals;

#[test]
fn parse_duration_supports_day_and_hour_units() {
    assert_eq!(parse_duration("7d").unwrap(), Duration::from_secs(604_800));
    assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
}

#[test]
fn report_groups_by_model() {
    let rows = build_report_rows(
        &[
            sample_summary("session-1", Some("gpt-5"), Some("medium"), 10),
            sample_summary("session-2", Some("gpt-5"), Some("high"), 15),
            sample_summary("session-3", Some("gpt-4.1"), Some("medium"), 8),
        ],
        GroupBy::Model,
    );

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].key, "gpt-4.1");
    assert_eq!(rows[0].total_tokens, 8);
    assert_eq!(rows[1].key, "gpt-5");
    assert_eq!(rows[1].total_tokens, 25);
}

#[test]
fn report_groups_by_effort() {
    let rows = build_report_rows(
        &[
            sample_summary("session-1", Some("gpt-5"), Some("medium"), 10),
            sample_summary("session-2", Some("gpt-5"), Some("high"), 15),
            sample_summary("session-3", Some("gpt-4.1"), Some("medium"), 8),
        ],
        GroupBy::Effort,
    );

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].key, "high");
    assert_eq!(rows[0].total_tokens, 15);
    assert_eq!(rows[1].key, "medium");
    assert_eq!(rows[1].total_tokens, 18);
}

#[test]
fn csv_field_escapes_quotes() {
    assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
}

fn sample_summary(
    session_id: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    total_tokens: u64,
) -> SessionSummary {
    SessionSummary {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        session_id: session_id.to_string(),
        started_at: "2026-06-17T12:00:00Z".to_string(),
        ended_at: Some("2026-06-17T12:05:00Z".to_string()),
        duration_ms: Some(300_000),
        invocation_mode: "exec".to_string(),
        session_source: "exec".to_string(),
        model: model.map(str::to_string),
        reasoning_effort: reasoning_effort.map(str::to_string),
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        cwd: Some("/workspace".to_string()),
        repo_root: Some(PathBuf::from("/workspace").display().to_string()),
        git: None,
        prompt_metadata: PromptMetadataSummary::default(),
        raw_event_path: "/workspace/events.jsonl".to_string(),
        rollout_path: None,
        usage_totals: UsageTotals {
            total_tokens,
            ..UsageTotals::default()
        },
        turn_counts: TurnCounts::default(),
        tool_summary: Default::default(),
        approval_summary: Default::default(),
        error_summary: Default::default(),
        changed_files_summary: ChangedFilesSummary::default(),
        resumed_from: None,
        forked_from: None,
    }
}
