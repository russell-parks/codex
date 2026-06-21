use std::path::PathBuf;
use std::time::Duration;

use pretty_assertions::assert_eq;

use crate::telemetry_cmd::GroupBy;
use crate::telemetry_cmd::build_report_insights;
use crate::telemetry_cmd::build_report_rows;
use crate::telemetry_cmd::build_report_rows_from_rollups;
use crate::telemetry_cmd::csv_field;
use crate::telemetry_cmd::parse_duration;
use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::ConfigSnapshotSummary;
use codex_local_telemetry::ConfigSourceSummary;
use codex_local_telemetry::DailyRollup;
use codex_local_telemetry::PromptMetadataSummary;
use codex_local_telemetry::RollupBucket;
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

#[test]
fn day_report_rows_use_rollup_totals() {
    let rows = build_report_rows_from_rollups(&[
        DailyRollup {
            schema_version: TELEMETRY_SCHEMA_VERSION,
            date: "2026-06-18".to_string(),
            totals: RollupBucket {
                sessions: 3,
                total_tokens: 42,
                cached_input_tokens: 7,
                reasoning_tokens: 5,
                tool_calls: 9,
                duration_ms: 600,
                ..RollupBucket::default()
            },
            by_model: Default::default(),
            by_effort: Default::default(),
            by_repo: Default::default(),
            by_mode: Default::default(),
            by_task_type: Default::default(),
        },
        DailyRollup {
            schema_version: TELEMETRY_SCHEMA_VERSION,
            date: "2026-06-17".to_string(),
            totals: RollupBucket {
                sessions: 1,
                total_tokens: 10,
                cached_input_tokens: 2,
                reasoning_tokens: 1,
                tool_calls: 3,
                duration_ms: 120,
                ..RollupBucket::default()
            },
            by_model: Default::default(),
            by_effort: Default::default(),
            by_repo: Default::default(),
            by_mode: Default::default(),
            by_task_type: Default::default(),
        },
    ]);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].key, "2026-06-18");
    assert_eq!(rows[0].sessions, 3);
    assert_eq!(rows[0].total_tokens, 42);
    assert_eq!(rows[1].key, "2026-06-17");
    assert_eq!(rows[1].cached_input_tokens, 2);
}

#[test]
fn report_insights_surface_expected_sessions() {
    let mut no_change = sample_summary("session-no-change", Some("gpt-5"), Some("medium"), 50);
    no_change.tool_summary.total_calls = 2;
    no_change.usage_totals.reasoning_tokens = 10;
    no_change.error_summary.error_count = 1;
    no_change.changed_files_summary = ChangedFilesSummary::default();

    let mut with_changes = sample_summary("session-with-changes", Some("gpt-5"), Some("high"), 20);
    with_changes.tool_summary.total_calls = 9;
    with_changes.changed_files_summary.paths = vec!["src/main.rs".to_string()];

    let mut aborted = sample_summary("session-aborted", Some("gpt-4.1"), Some("low"), 30);
    aborted.turn_counts.aborted = 1;

    let insights = build_report_insights(&[no_change.clone(), with_changes, aborted.clone()]);

    assert_eq!(
        insights.high_token_without_changes[0].session_id,
        no_change.session_id
    );
    assert_eq!(
        insights.high_reasoning_sessions[0].session_id,
        no_change.session_id
    );
    assert_eq!(
        insights.many_tool_call_sessions[0].session_id,
        "session-with-changes"
    );
    assert_eq!(
        insights.high_token_failures[0].session_id,
        no_change.session_id
    );
    assert_eq!(
        insights.high_token_failures[1].session_id,
        aborted.session_id
    );
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
        final_outcome: Some("completed".to_string()),
        abort_reason: None,
        exit_status_code: Some(0),
        invocation_mode: "exec".to_string(),
        session_source: "exec".to_string(),
        model: model.map(str::to_string),
        reasoning_effort: reasoning_effort.map(str::to_string),
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        active_profile: Some("safe".to_string()),
        cwd: Some("/workspace".to_string()),
        repo_root: Some(PathBuf::from("/workspace").display().to_string()),
        git: None,
        config_snapshot: Some(ConfigSnapshotSummary {
            config_sources: vec![ConfigSourceSummary {
                kind: "user".to_string(),
                source: "user (/workspace/.codex/config.toml)".to_string(),
                profile: Some("safe".to_string()),
            }],
            developer_instructions_loaded: true,
            user_instructions_loaded: false,
            user_instruction_source: None,
            project_instructions_loaded: false,
            project_instruction_sources: Vec::new(),
        }),
        prompt_metadata: PromptMetadataSummary::default(),
        raw_event_path: "/workspace/events.jsonl".to_string(),
        rollout_path: None,
        usage_totals: UsageTotals {
            total_tokens,
            ..UsageTotals::default()
        },
        turn_counts: TurnCounts::default(),
        tool_summary: Default::default(),
        runtime_summary: Default::default(),
        task_types: vec!["regular".to_string()],
        approval_summary: Default::default(),
        error_summary: Default::default(),
        changed_files_summary: ChangedFilesSummary::default(),
        resumed_from: None,
        forked_from: None,
    }
}
