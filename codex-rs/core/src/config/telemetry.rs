use codex_config::types::LocalTelemetryConfig;
use codex_config::types::TelemetryConfig;
use codex_config::types::TelemetryConfigToml;

pub(crate) fn resolve_config(config: TelemetryConfigToml) -> TelemetryConfig {
    let local = config.local.unwrap_or_default();
    TelemetryConfig {
        local: LocalTelemetryConfig {
            enabled: local.enabled.unwrap_or(true),
            directory: local
                .directory
                .unwrap_or_else(|| "~/.codex/telemetry".to_string()),
            retention_days: local.retention_days.unwrap_or(90),
            log_user_prompt: local.log_user_prompt.unwrap_or(false),
            log_assistant_text: local.log_assistant_text.unwrap_or(false),
            log_tool_output: local.log_tool_output.unwrap_or(false),
            log_diffs: local.log_diffs.unwrap_or(false),
            hash_prompts: local.hash_prompts.unwrap_or(true),
            capture_session: local.capture_session.unwrap_or(true),
            capture_turns: local.capture_turns.unwrap_or(true),
            capture_usage: local.capture_usage.unwrap_or(true),
            capture_tool_calls: local.capture_tool_calls.unwrap_or(true),
            capture_approvals: local.capture_approvals.unwrap_or(true),
            capture_git: local.capture_git.unwrap_or(true),
            capture_config_snapshot: local.capture_config_snapshot.unwrap_or(true),
            capture_errors: local.capture_errors.unwrap_or(true),
            write_run_summary: local.write_run_summary.unwrap_or(true),
            write_daily_rollups: local.write_daily_rollups.unwrap_or(true),
        },
    }
}
