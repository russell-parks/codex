#![cfg(not(target_os = "windows"))]

use anyhow::Result;
use codex_local_telemetry::SessionSummary;
use core_test_support::fs_wait;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex_exec::test_codex_exec;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;

const TELEMETRY_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_writes_local_telemetry_artifacts() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_response_created("resp1"),
        responses::ev_assistant_message("m1", "fixture hello"),
        responses::ev_completed("resp1"),
    ]);
    responses::mount_sse_once(&server, body).await;

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("-c")
        .arg("telemetry.local.directory=\"telemetry\"")
        .arg("small telemetry test prompt")
        .assert()
        .success()
        .stdout(contains("fixture hello"));

    let summary = wait_for_single_local_telemetry_summary(test.home_path()).await?;
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

async fn wait_for_single_local_telemetry_summary(codex_home: &Path) -> Result<SessionSummary> {
    let runs_dir = codex_home.join("telemetry/runs");
    let path = fs_wait::wait_for_matching_file(runs_dir, TELEMETRY_TIMEOUT, |path| {
        path.extension().and_then(|ext| ext.to_str()) == Some("json")
    })
    .await?;
    let summary = serde_json::from_str::<SessionSummary>(&std::fs::read_to_string(path)?)?;
    Ok(summary)
}
