use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use codex_local_telemetry::SessionSummary;
use core_test_support::skip_if_no_network;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;
use tokio::time::sleep;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const CLI_TIMEOUT: Duration = Duration::from_secs(30);

fn repo_root() -> Result<PathBuf> {
    Ok(codex_utils_cargo_bin::repo_root()?)
}

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[cfg(unix)]
fn interactive_codex_command(codex_home: &Path, args: &[String]) -> Result<assert_cmd::Command> {
    let codex_bin = codex_utils_cargo_bin::cargo_bin("codex")?;
    let mut cmd = if cfg!(target_os = "macos") {
        let mut cmd = assert_cmd::Command::new("script");
        cmd.arg("-q").arg("/dev/null").arg(codex_bin);
        cmd.args(args);
        cmd
    } else {
        let command = std::iter::once(codex_bin.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(|arg| shell_quote(&arg))
            .collect::<Vec<_>>()
            .join(" ");
        let mut cmd = assert_cmd::Command::new("script");
        cmd.args(["-q", "-e", "-c", &command, "/dev/null"]);
        cmd
    };
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn sse_response() -> String {
    concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-1\"}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"fixture hello\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\"}}\n\n"
    )
    .to_string()
}

async fn mount_responses_once(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_response()),
        )
        .expect(1)
        .mount(server)
        .await;
}

async fn wait_for_single_local_telemetry_summary(telemetry_root: &Path) -> Result<SessionSummary> {
    let runs_dir = telemetry_root.join("runs");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);

    loop {
        if let Ok(entries) = std::fs::read_dir(&runs_dir) {
            let mut json_paths = entries
                .filter_map(|entry| entry.ok().map(|value| value.path()))
                .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
                .collect::<Vec<_>>();
            json_paths.sort();
            if let Some(path) = json_paths.first() {
                let summary =
                    serde_json::from_str::<SessionSummary>(&std::fs::read_to_string(path)?)?;
                return Ok(summary);
            }
        }

        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "timed out waiting for telemetry summary under {}",
                runs_dir.display()
            );
        }

        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_cli_writes_local_telemetry_artifacts() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    mount_responses_once(&server).await;
    let home = TempDir::new()?;
    let telemetry_root = home.path().join("telemetry");

    let mut cmd = codex_command(home.path())?;
    let assert = cmd
        .timeout(CLI_TIMEOUT)
        .args([
            "exec",
            "--skip-git-repo-check",
            "-c",
            &format!("openai_base_url=\"{}/v1\"", server.uri()),
            "-c",
            "telemetry.local.directory=\"telemetry\"",
            "-C",
            repo_root()?
                .to_str()
                .unwrap_or_else(|| panic!("repo root should be utf-8")),
            "small telemetry test prompt",
        ])
        .env("OPENAI_API_KEY", "dummy")
        .assert();
    assert.success().stdout(contains("fixture hello"));

    let summary = wait_for_single_local_telemetry_summary(&telemetry_root).await?;
    assert_eq!(summary.invocation_mode, "exec");
    assert_eq!(summary.session_source, "exec");
    assert_eq!(summary.final_outcome.as_deref(), Some("completed"));
    assert_eq!(summary.abort_reason, None);
    assert_eq!(summary.exit_status_code, Some(0));
    assert_eq!(summary.prompt_metadata.prompt_text, None);
    assert!(Path::new(&summary.raw_event_path).exists());
    assert!(summary.raw_event_path.contains("/telemetry/events/"));

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fully interactive terminal emulator; local interactive telemetry is covered by core session and extension tests"]
async fn interactive_cli_writes_local_telemetry_artifacts() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = MockServer::start().await;
    mount_responses_once(&server).await;
    let home = TempDir::new()?;
    let telemetry_root = home.path().join("telemetry");

    let repo_root = repo_root()?;
    let mut cmd = interactive_codex_command(
        home.path(),
        &[
            String::from("-c"),
            format!("openai_base_url=\"{}/v1\"", server.uri()),
            String::from("-c"),
            String::from("telemetry.local.directory=\"telemetry\""),
            String::from("-C"),
            repo_root
                .to_str()
                .expect("repo root should be utf-8")
                .to_string(),
            String::from("--"),
            String::from("interactive telemetry prompt"),
        ],
    )?;
    cmd.timeout(CLI_TIMEOUT)
        .env("OPENAI_API_KEY", "dummy")
        .env("TERM", "xterm-256color")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(contains("fixture hello"));

    let summary = wait_for_single_local_telemetry_summary(&telemetry_root).await?;
    assert_eq!(summary.invocation_mode, "interactive");
    assert_eq!(summary.session_source, "cli");
    assert_eq!(summary.final_outcome.as_deref(), Some("completed"));
    assert_eq!(summary.abort_reason, None);
    assert_eq!(summary.exit_status_code, Some(0));
    assert_eq!(summary.prompt_metadata.prompt_text, None);
    assert!(Path::new(&summary.raw_event_path).exists());

    Ok(())
}

#[test]
fn telemetry_status_reports_local_configuration() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
[telemetry.local]
directory = "telemetry"
retention_days = 45
log_user_prompt = false
hash_prompts = true
"#,
    )?;
    std::fs::create_dir_all(home.path().join("telemetry/events/2026/06/20"))?;
    std::fs::write(
        home.path()
            .join("telemetry/events/2026/06/20/session-1.jsonl"),
        r#"{"schema_version":1,"timestamp":"2026-06-20T12:00:00Z","session_id":"session-1","turn_id":null,"event_type":"session_started","payload":{}}"#,
    )?;

    let mut cmd = codex_command(home.path())?;
    cmd.args(["telemetry", "status"])
        .assert()
        .success()
        .stdout(contains("enabled: true"))
        .stdout(contains("directory: "))
        .stdout(contains("retention_days: 45"))
        .stdout(contains("otel_configured: false"))
        .stdout(contains("privacy.log_user_prompt: false"))
        .stdout(contains("privacy.hash_prompts: true"));

    Ok(())
}

#[test]
fn telemetry_status_reports_when_otel_is_configured_separately() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
[telemetry.local]
directory = "telemetry"

[otel]
environment = "prod"
"#,
    )?;

    let mut cmd = codex_command(home.path())?;
    cmd.args(["telemetry", "status"])
        .assert()
        .success()
        .stdout(contains("otel_configured: true"));

    Ok(())
}

#[test]
fn telemetry_doctor_reports_store_health() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
[telemetry.local]
directory = "telemetry"
"#,
    )?;
    std::fs::create_dir_all(home.path().join("telemetry/events/2026/06/20"))?;
    std::fs::create_dir_all(home.path().join("telemetry/runs"))?;
    std::fs::create_dir_all(home.path().join("telemetry/rollups"))?;
    std::fs::write(
        home.path()
            .join("telemetry/events/2026/06/20/session-1.jsonl"),
        r#"{"schema_version":1,"timestamp":"2026-06-20T12:00:00Z","session_id":"session-1","turn_id":null,"event_type":"session_started","payload":{}}"#,
    )?;
    std::fs::write(
        home.path()
            .join("telemetry/events/2026/06/20/orphaned.jsonl"),
        r#"{"schema_version":1,"timestamp":"2026-06-20T12:05:00Z","session_id":"orphaned","turn_id":null,"event_type":"session_started","payload":{}}"#,
    )?;
    std::fs::write(
        home.path().join("telemetry/runs/session-1.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "session_id": "session-1",
            "started_at": "2026-06-20T12:00:00Z",
            "ended_at": "2026-06-20T12:01:00Z",
            "duration_ms": 60000,
            "invocation_mode": "exec",
            "session_source": "exec",
            "model": "gpt-5",
            "reasoning_effort": "medium",
            "approval_policy": "on-request",
            "sandbox_mode": "workspace-write",
            "active_profile": "safe",
            "cwd": "/workspace",
            "repo_root": "/workspace",
            "git": null,
            "config_snapshot": null,
            "prompt_metadata": {
                "prompt_byte_length": 4,
                "prompt_token_estimate": null,
                "prompt_sha256": "abcd",
                "prompt_text": null
            },
            "raw_event_path": home.path().join("telemetry/events/2026/06/20/missing.jsonl").display().to_string(),
            "rollout_path": null,
            "usage_totals": {
                "input_tokens": 1,
                "output_tokens": 1,
                "reasoning_tokens": 0,
                "cached_input_tokens": 0,
                "total_tokens": 2
            },
            "turn_counts": {
                "started": 1,
                "completed": 1,
                "aborted": 0,
                "errored": 0
            },
            "tool_summary": {
                "total_calls": 0,
                "success_count": 0,
                "failure_count": 0
            },
            "approval_summary": {
                "total_requests": 0,
                "approved_count": 0,
                "denied_count": 0
            },
            "error_summary": {
                "error_count": 0,
                "last_error": null
            },
            "changed_files_summary": {
                "paths": [],
                "counts_by_extension": {},
                "insertions": null,
                "deletions": null
            },
            "resumed_from": null,
            "forked_from": null
        }))?,
    )?;
    std::fs::write(
        home.path().join("telemetry/rollups/2026-06-20.json"),
        r#"{"schema_version":1,"date":"2026-06-20","totals":{"sessions":1,"turns":1,"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_tokens":0,"total_tokens":2,"tool_calls":0,"approvals":0,"failures":0,"duration_ms":60000},"by_model":{},"by_effort":{},"by_repo":{},"by_mode":{}}"#,
    )?;

    let mut cmd = codex_command(home.path())?;
    cmd.args(["telemetry", "doctor"])
        .assert()
        .success()
        .stdout(contains("directory_exists: true"))
        .stdout(contains("summaries: 1"))
        .stdout(contains("event_files: 2"))
        .stdout(contains("rollups: 1"))
        .stdout(contains("missing_event_files: 1"))
        .stdout(contains("orphaned_event_files: 2"));

    Ok(())
}

#[test]
fn telemetry_show_and_report_surface_seeded_summaries() -> Result<()> {
    let home = TempDir::new()?;
    write_telemetry_config(home.path())?;

    let model_a_started_at = telemetry_timestamp_days_ago(1, 12, 0);
    let model_a_ended_at = telemetry_timestamp_days_ago(1, 12, 1);
    write_summary_json(
        home.path(),
        sample_summary_json(SampleSummaryInput {
            codex_home: home.path(),
            session_id: "session-model-a",
            started_at: &model_a_started_at,
            ended_at: &model_a_ended_at,
            model: "gpt-5",
            effort: "medium",
            total_tokens: 100,
            tool_calls: 7,
            reasoning_tokens: 2,
            changed_files: &["src/main.rs"],
        }),
    )?;
    let model_b_started_at = telemetry_timestamp_days_ago(1, 13, 0);
    let model_b_ended_at = telemetry_timestamp_days_ago(1, 13, 1);
    write_summary_json(
        home.path(),
        sample_summary_json(SampleSummaryInput {
            codex_home: home.path(),
            session_id: "session-model-b",
            started_at: &model_b_started_at,
            ended_at: &model_b_ended_at,
            model: "gpt-4.1",
            effort: "high",
            total_tokens: 40,
            tool_calls: 2,
            reasoning_tokens: 0,
            changed_files: &[],
        }),
    )?;

    let mut show_cmd = codex_command(home.path())?;
    show_cmd
        .args(["telemetry", "show", "session-model-a"])
        .assert()
        .success()
        .stdout(contains("session_id: session-model-a"))
        .stdout(contains("invocation_mode: exec"))
        .stdout(contains("model: gpt-5"))
        .stdout(contains("reasoning_effort: medium"))
        .stdout(contains("total_tokens: 100"));

    let mut model_report_cmd = codex_command(home.path())?;
    model_report_cmd
        .args([
            "telemetry",
            "report",
            "--since",
            "7d",
            "--group-by",
            "model",
        ])
        .assert()
        .success()
        .stdout(contains("gpt-5"))
        .stdout(contains("gpt-4.1"))
        .stdout(contains("High reasoning-token sessions:"));

    let mut effort_report_cmd = codex_command(home.path())?;
    effort_report_cmd
        .args([
            "telemetry",
            "report",
            "--since",
            "7d",
            "--group-by",
            "effort",
        ])
        .assert()
        .success()
        .stdout(contains("medium"))
        .stdout(contains("high"));

    Ok(())
}

#[test]
fn telemetry_export_and_prune_use_seeded_store() -> Result<()> {
    let home = TempDir::new()?;
    write_telemetry_config(home.path())?;

    let old_started_at = telemetry_timestamp_days_ago(14, 12, 0);
    let old_ended_at = telemetry_timestamp_days_ago(14, 12, 1);
    write_summary_json(
        home.path(),
        sample_summary_json(SampleSummaryInput {
            codex_home: home.path(),
            session_id: "session-old",
            started_at: &old_started_at,
            ended_at: &old_ended_at,
            model: "gpt-5",
            effort: "medium",
            total_tokens: 10,
            tool_calls: 1,
            reasoning_tokens: 0,
            changed_files: &[],
        }),
    )?;
    let new_started_at = telemetry_timestamp_days_ago(1, 12, 0);
    let new_ended_at = telemetry_timestamp_days_ago(1, 12, 1);
    write_summary_json(
        home.path(),
        sample_summary_json(SampleSummaryInput {
            codex_home: home.path(),
            session_id: "session-new",
            started_at: &new_started_at,
            ended_at: &new_ended_at,
            model: "gpt-5",
            effort: "medium",
            total_tokens: 25,
            tool_calls: 3,
            reasoning_tokens: 0,
            changed_files: &["src/lib.rs"],
        }),
    )?;
    write_event_jsonl(
        home.path(),
        &telemetry_date_path(&old_started_at),
        "session-old",
        &old_ended_at,
    )?;
    write_event_jsonl(
        home.path(),
        &telemetry_date_path(&new_started_at),
        "session-new",
        &new_ended_at,
    )?;
    std::fs::create_dir_all(home.path().join("telemetry/rollups"))?;
    let old_rollup_date = telemetry_date(&old_started_at);
    std::fs::write(
        home.path()
            .join(format!("telemetry/rollups/{old_rollup_date}.json")),
        format!(
            r#"{{"schema_version":1,"date":"{old_rollup_date}","totals":{{"sessions":1,"turns":1,"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_tokens":0,"total_tokens":2,"tool_calls":0,"approvals":0,"failures":0,"duration_ms":60000}},"by_model":{{}},"by_effort":{{}},"by_repo":{{}},"by_mode":{{}}}}"#
        ),
    )?;

    let export_path = home.path().join("telemetry-export.csv");
    let mut export_cmd = codex_command(home.path())?;
    export_cmd
        .args([
            "telemetry",
            "export",
            "--since",
            "30d",
            "--format",
            "csv",
            "--output",
            export_path.to_str().expect("export path should be utf-8"),
        ])
        .assert()
        .success();
    let csv = std::fs::read_to_string(&export_path)?;
    assert!(csv.contains("\"session-new\""));

    let mut prune_cmd = codex_command(home.path())?;
    prune_cmd
        .args(["telemetry", "prune", "--older-than", "7d"])
        .assert()
        .success()
        .stdout(contains("removed_summaries: 1"))
        .stdout(contains("removed_event_files: 1"))
        .stdout(contains("removed_rollups: 1"));

    assert!(home.path().join("telemetry/runs/session-new.json").exists());
    assert!(!home.path().join("telemetry/runs/session-old.json").exists());

    Ok(())
}

#[test]
fn telemetry_list_filters_by_model_and_repo() -> Result<()> {
    let home = TempDir::new()?;
    write_telemetry_config(home.path())?;

    write_summary_json(
        home.path(),
        sample_summary_json(SampleSummaryInput {
            codex_home: home.path(),
            session_id: "session-gpt5",
            started_at: "2026-06-20T12:00:00Z",
            ended_at: "2026-06-20T12:01:00Z",
            model: "gpt-5",
            effort: "medium",
            total_tokens: 10,
            tool_calls: 1,
            reasoning_tokens: 0,
            changed_files: &[],
        }),
    )?;
    let mut other_repo = sample_summary_json(SampleSummaryInput {
        codex_home: home.path(),
        session_id: "session-other-repo",
        started_at: "2026-06-20T13:00:00Z",
        ended_at: "2026-06-20T13:01:00Z",
        model: "gpt-4.1",
        effort: "high",
        total_tokens: 20,
        tool_calls: 2,
        reasoning_tokens: 0,
        changed_files: &[],
    });
    other_repo["repo_root"] = serde_json::Value::String("/elsewhere".to_string());
    other_repo["cwd"] = serde_json::Value::String("/elsewhere".to_string());
    write_summary_json(home.path(), other_repo)?;

    let mut cmd = codex_command(home.path())?;
    cmd.args([
        "telemetry",
        "list",
        "--repo",
        "/workspace",
        "--model",
        "gpt-5",
    ])
    .assert()
    .success()
    .stdout(contains("session-gpt5"))
    .stdout(predicates::str::contains("session-other-repo").not());

    Ok(())
}

fn write_telemetry_config(codex_home: &Path) -> Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        r#"
[telemetry.local]
directory = "telemetry"
"#,
    )?;
    Ok(())
}

fn write_summary_json(codex_home: &Path, summary: serde_json::Value) -> Result<()> {
    let session_id = summary["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("session_id should be present"));
    let runs_dir = codex_home.join("telemetry/runs");
    std::fs::create_dir_all(&runs_dir)?;
    std::fs::write(
        runs_dir.join(format!("{session_id}.json")),
        serde_json::to_string_pretty(&summary)?,
    )?;
    Ok(())
}

fn write_event_jsonl(
    codex_home: &Path,
    date_path: &str,
    session_id: &str,
    timestamp: &str,
) -> Result<()> {
    let dir = codex_home.join("telemetry/events").join(date_path);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(format!("{session_id}.jsonl")),
        format!(
            "{{\"schema_version\":1,\"timestamp\":\"{timestamp}\",\"session_id\":\"{session_id}\",\"turn_id\":null,\"event_type\":\"session_completed\",\"payload\":{{}}}}\n"
        ),
    )?;
    Ok(())
}

struct SampleSummaryInput<'a> {
    codex_home: &'a Path,
    session_id: &'a str,
    started_at: &'a str,
    ended_at: &'a str,
    model: &'a str,
    effort: &'a str,
    total_tokens: u64,
    tool_calls: u64,
    reasoning_tokens: u64,
    changed_files: &'a [&'a str],
}

fn sample_summary_json(input: SampleSummaryInput<'_>) -> serde_json::Value {
    let SampleSummaryInput {
        codex_home,
        session_id,
        started_at,
        ended_at,
        model,
        effort,
        total_tokens,
        tool_calls,
        reasoning_tokens,
        changed_files,
    } = input;
    let raw_event_path = raw_event_path_for(codex_home, started_at, session_id);
    serde_json::json!({
        "schema_version": 1,
        "session_id": session_id,
        "started_at": started_at,
        "ended_at": ended_at,
        "duration_ms": 60000,
        "invocation_mode": "exec",
        "session_source": "exec",
        "model": model,
        "reasoning_effort": effort,
        "approval_policy": "on-request",
        "sandbox_mode": "workspace-write",
        "active_profile": "safe",
        "cwd": "/workspace",
        "repo_root": "/workspace",
        "git": null,
        "config_snapshot": null,
        "prompt_metadata": {
            "prompt_byte_length": 4,
            "prompt_token_estimate": null,
            "prompt_sha256": "abcd",
            "prompt_text": null
        },
        "raw_event_path": raw_event_path.display().to_string(),
        "rollout_path": null,
        "usage_totals": {
            "input_tokens": total_tokens / 2,
            "output_tokens": total_tokens / 4,
            "reasoning_tokens": reasoning_tokens,
            "cached_input_tokens": total_tokens / 10,
            "total_tokens": total_tokens
        },
        "turn_counts": {
            "started": 1,
            "completed": 1,
            "aborted": 0,
            "errored": 0
        },
        "tool_summary": {
            "total_calls": tool_calls,
            "success_count": tool_calls,
            "failure_count": 0
        },
        "approval_summary": {
            "total_requests": 0,
            "approved_count": 0,
            "denied_count": 0
        },
        "error_summary": {
            "error_count": 0,
            "last_error": null
        },
        "changed_files_summary": {
            "paths": changed_files,
            "counts_by_extension": changed_files.iter().fold(serde_json::Map::new(), |mut counts, path| {
                let ext = PathBuf::from(path)
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_string();
                let next = counts
                    .get(&ext)
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
                    + 1;
                counts.insert(ext, serde_json::Value::from(next));
                counts
            }),
            "insertions": 3,
            "deletions": 1
        },
        "resumed_from": null,
        "forked_from": null
    })
}

fn telemetry_timestamp_days_ago(days_ago: i64, hour: u32, minute: u32) -> String {
    let date = (Utc::now() - ChronoDuration::days(days_ago)).date_naive();
    format!("{date}T{hour:02}:{minute:02}:00Z")
}

fn telemetry_date(timestamp: &str) -> &str {
    timestamp
        .split('T')
        .next()
        .unwrap_or_else(|| panic!("timestamp should include date"))
}

fn telemetry_date_path(timestamp: &str) -> String {
    telemetry_date(timestamp).replace('-', "/")
}

fn raw_event_path_for(codex_home: &Path, started_at: &str, session_id: &str) -> PathBuf {
    let date = telemetry_date(started_at);
    let mut parts = date.split('-');
    let year = parts
        .next()
        .unwrap_or_else(|| panic!("year should be present"));
    let month = parts
        .next()
        .unwrap_or_else(|| panic!("month should be present"));
    let day = parts
        .next()
        .unwrap_or_else(|| panic!("day should be present"));
    std::fs::canonicalize(codex_home)
        .unwrap_or_else(|_| codex_home.to_path_buf())
        .join("telemetry/events")
        .join(year)
        .join(month)
        .join(day)
        .join(format!("{session_id}.jsonl"))
}
