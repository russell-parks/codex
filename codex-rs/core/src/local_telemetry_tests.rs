use codex_local_telemetry::RuntimeSummary;
use codex_otel::MetricsClient;
use codex_otel::MetricsConfig;
use codex_otel::SessionTelemetry;
use codex_otel::TelemetryAuthMode;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use opentelemetry_sdk::metrics::InMemoryMetricExporter;
use pretty_assertions::assert_eq;
use std::time::Duration;

use super::invocation_mode_for_session_source;
use super::runtime_summary_patch;

#[test]
fn invocation_mode_uses_interactive_label_for_cli_sessions() {
    assert_eq!(
        invocation_mode_for_session_source(&SessionSource::Cli),
        "interactive"
    );
}

#[test]
fn invocation_mode_keeps_exec_label_for_exec_sessions() {
    assert_eq!(
        invocation_mode_for_session_source(&SessionSource::Exec),
        "exec"
    );
}

#[test]
fn invocation_mode_uses_mcp_server_label_for_mcp_sessions() {
    assert_eq!(
        invocation_mode_for_session_source(&SessionSource::Mcp),
        "mcp-server"
    );
}

#[test]
fn runtime_summary_patch_uses_http_byte_totals_without_websockets() {
    let exporter = InMemoryMetricExporter::default();
    let metrics = MetricsClient::new(
        MetricsConfig::in_memory("test", "codex-core", env!("CARGO_PKG_VERSION"), exporter)
            .with_runtime_reader(),
    )
    .expect("metrics client should initialize");
    let telemetry = SessionTelemetry::new(
        ThreadId::new(),
        "gpt-5",
        "gpt-5",
        None,
        None,
        Some(TelemetryAuthMode::ApiKey),
        "test_originator".to_string(),
        true,
        "tty".to_string(),
        SessionSource::Cli,
    )
    .with_metrics(metrics);
    telemetry.reset_runtime_metrics();
    telemetry.record_api_request(
        /*attempt*/ 1,
        Some(200),
        None,
        Duration::from_millis(50),
        /*request_body_bytes*/ Some(128),
        /*response_body_bytes*/ Some(512),
        /*auth_header_attached*/ false,
        /*auth_header_name*/ None,
        /*retry_after_unauthorized*/ false,
        /*recovery_mode*/ None,
        /*recovery_phase*/ None,
        "/responses",
        /*request_id*/ None,
        /*cf_ray*/ None,
        /*auth_error*/ None,
        /*auth_error_code*/ None,
        /*agent_identity_telemetry*/ None,
    );
    telemetry.record_sse_bytes_read(1024);

    assert_eq!(
        runtime_summary_patch(&telemetry),
        Some(RuntimeSummary {
            api_request_count: 0,
            retry_count: 0,
            latest_rate_limits: None,
            bytes_read: Some(1_536),
            bytes_written: Some(128),
        })
    );
}

#[test]
fn runtime_summary_patch_skips_partial_websocket_byte_totals() {
    let exporter = InMemoryMetricExporter::default();
    let metrics = MetricsClient::new(
        MetricsConfig::in_memory("test", "codex-core", env!("CARGO_PKG_VERSION"), exporter)
            .with_runtime_reader(),
    )
    .expect("metrics client should initialize");
    let telemetry = SessionTelemetry::new(
        ThreadId::new(),
        "gpt-5",
        "gpt-5",
        None,
        None,
        Some(TelemetryAuthMode::ApiKey),
        "test_originator".to_string(),
        true,
        "tty".to_string(),
        SessionSource::Cli,
    )
    .with_metrics(metrics);
    telemetry.reset_runtime_metrics();
    telemetry.record_api_request(
        /*attempt*/ 1,
        Some(200),
        None,
        Duration::from_millis(50),
        /*request_body_bytes*/ Some(128),
        /*response_body_bytes*/ Some(512),
        /*auth_header_attached*/ false,
        /*auth_header_name*/ None,
        /*retry_after_unauthorized*/ false,
        /*recovery_mode*/ None,
        /*recovery_phase*/ None,
        "/responses",
        /*request_id*/ None,
        /*cf_ray*/ None,
        /*auth_error*/ None,
        /*auth_error_code*/ None,
        /*agent_identity_telemetry*/ None,
    );
    telemetry.record_websocket_request(
        Duration::from_millis(75),
        /*error*/ None,
        /*connection_reused*/ false,
        /*agent_identity_telemetry*/ None,
    );

    assert_eq!(runtime_summary_patch(&telemetry), None);
}
