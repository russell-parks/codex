# codex-local-telemetry

`codex-local-telemetry` is Codex's local, user-owned telemetry store.

It is intentionally separate from `codex-otel`:

- Local telemetry writes append-only JSONL events and per-session summaries to
  the user's Codex state directory.
- OpenTelemetry remains the existing export path for users who want external
  tracing, logs, or metrics.
- Enabling local telemetry does not require a network exporter, admin key, or
  an external wrapper around the `codex` CLI.

## Storage layout

By default Codex stores local telemetry under `~/.codex/telemetry`:

```text
~/.codex/telemetry/
  events/YYYY/MM/DD/<session-id>.jsonl
  runs/<session-id>.json
```

Raw events are append-only JSONL records:

```json
{
  "schema_version": 1,
  "timestamp": "2026-06-17T18:20:00Z",
  "session_id": "session-123",
  "turn_id": "turn-1",
  "event_type": "token_usage_checkpoint",
  "payload": {}
}
```

Session summaries are compact derived views that power `codex telemetry`
queries without re-reading every raw event file.

## Config

Local telemetry is configured from the user-level `~/.codex/config.toml`:

```toml
[telemetry.local]
enabled = true
directory = "~/.codex/telemetry"
retention_days = 90

# Privacy defaults
log_user_prompt = false
log_assistant_text = false
log_tool_output = false
log_diffs = false
hash_prompts = true

# Event classes
capture_session = true
capture_turns = true
capture_usage = true
capture_tool_calls = true
capture_approvals = true
capture_git = true
capture_config_snapshot = true
capture_errors = true

# Derived artifacts
write_run_summary = true
```

Privacy defaults are conservative:

- Full user prompts are not stored unless `log_user_prompt = true`.
- Prompt hashes can still be recorded with `hash_prompts = true`.
- Assistant text, tool output, diffs, file contents, and command output are not
  persisted by default.

## CLI

Use the `codex telemetry` subcommands to inspect the local store:

```text
codex telemetry status
codex telemetry list --since 7d
codex telemetry show <session-id>
codex telemetry report --since 7d --group-by model
codex telemetry export --since 30d --format csv --output telemetry.csv
codex telemetry prune --older-than 90d
codex telemetry doctor
```

## Relationship to OpenTelemetry

Local telemetry and OpenTelemetry can be enabled independently:

- `telemetry.local` controls local on-disk capture for the current user.
- `[otel]` continues to control external OpenTelemetry export.

The `codex telemetry status` command reports whether OTel is configured
separately so users can distinguish local storage from remote export.
