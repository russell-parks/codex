# Local Telemetry Capture Design

## Goal

Add a production-quality local telemetry capture path for Codex CLI usage that answers:

1. What did the user ask Codex to do?
2. What did Codex actually consume?

This first slice implements capture only:

- user-level config for local telemetry
- append-only local raw events
- per-session derived summaries
- privacy-preserving defaults
- robust no-op and failure-safe behavior
- support for both interactive and non-interactive `codex exec`

This slice does not implement:

- `codex telemetry ...` CLI analysis commands
- daily rollups
- cloud telemetry as the primary path
- changes to existing rollout persistence under `~/.codex/sessions`
- changes to existing OpenTelemetry behavior

## Non-Goals

- Replacing or redefining the existing rollout JSONL format
- Using local telemetry as the authoritative model usage source when existing rollout/session usage data already exists
- Logging full prompts, assistant text, tool output, command output, diffs, or file contents by default
- Storing telemetry inside project repositories by default

## Requirements

### Functional

- Store local telemetry under the Codex user state directory, defaulting to `~/.codex/telemetry`
- Work for both interactive sessions and `codex exec`
- Preserve existing session and rollout behavior under `~/.codex/sessions`
- Reuse or mirror existing usage facts such as `token_count`, `last_token_usage`, and `total_token_usage` where available
- Write append-only JSONL raw events plus a derived per-session summary
- Never require external wrapping, admin keys, or network export

### Privacy

- Sensitive content capture is opt-in and clearly named
- Privacy defaults exclude:
  - full prompt text
  - assistant text
  - tool output
  - command output
  - diffs
  - file contents
- Prompt hashing defaults on when prompt text logging is off
- Config snapshots exclude secrets and secret-bearing values

### Reliability

- Telemetry write failures must never fail a Codex run
- Disabled telemetry must use a true no-op path
- Partial capture is acceptable when some facts are unavailable, but the schema should remain stable

## High-Level Approach

Implement local telemetry as a new extension-backed capture layer with a narrow `core` bridge for session facts that are only known during session creation and shutdown.

This keeps the hot-path telemetry logic out of `codex-core` as much as possible while reusing existing lifecycle hooks:

- thread lifecycle
- turn lifecycle
- token usage contributor
- tool lifecycle

The local telemetry store is a second sink that complements existing OpenTelemetry support. It does not replace, disable, or reinterpret OTel export behavior.

## Proposed Module Layout

### `codex-config`

Add local telemetry config types:

- `TelemetryConfigToml`
- `LocalTelemetryConfigToml`
- `TelemetryConfig`
- `LocalTelemetryConfig`

Responsibilities:

- TOML schema
- defaults
- config schema generation
- resolved effective config values

### New crate: `codex-local-telemetry`

Responsibilities:

- raw event schema
- summary schema
- privacy filtering helpers
- append-only event writer
- summary builder
- no-op writer implementation
- local filesystem path layout helpers

This crate should not depend on OTel.

### New crate: `codex-ext-local-telemetry`

Responsibilities:

- extension installation
- lifecycle capture integration
- translating core lifecycle inputs into local telemetry schema events

This crate should register:

- `ThreadLifecycleContributor`
- `TurnLifecycleContributor`
- `TokenUsageContributor`
- `ToolLifecycleContributor`

### `codex-core`

Responsibilities:

- install the extension during normal startup
- provide session start and session end facts not already surfaced through extension inputs
- expose enough metadata for interactive and exec modes to share one local telemetry path

### Future CLI analysis work

Deferred to later slices:

- `codex telemetry status`
- `codex telemetry list`
- `codex telemetry show`
- `codex telemetry report`
- `codex telemetry export`
- `codex telemetry prune`
- `codex telemetry doctor`

## Config Design

Add user-level config:

```toml
[telemetry.local]
enabled = true
directory = "~/.codex/telemetry"
retention_days = 90
log_user_prompt = false
log_assistant_text = false
log_tool_output = false
log_diffs = false
hash_prompts = true
capture_session = true
capture_turns = true
capture_usage = true
capture_tool_calls = true
capture_approvals = true
capture_git = true
capture_config_snapshot = true
capture_errors = true
write_run_summary = true
```

Notes:

- The exact type names should follow existing config naming conventions.
- The first slice should omit `write_daily_rollups`; rollups belong to the later analysis slice.
- `directory` should resolve under `CODEX_HOME` by default and should not point into the active repo unless the user explicitly configures that.

Recommended defaults for this slice:

- `enabled = true`
- `directory = "~/.codex/telemetry"`
- `retention_days = 90`
- content logging flags default `false`
- `hash_prompts = true`
- capture flags default `true`
- `write_run_summary = true`

## Storage Layout

### Raw events

Path:

`~/.codex/telemetry/events/YYYY/MM/DD/<session-id>.jsonl`

Each line:

```json
{
  "schema_version": 1,
  "timestamp": "2026-06-17T12:34:56Z",
  "session_id": "uuid-or-session-id",
  "turn_id": "optional-turn-id",
  "event_type": "session_started",
  "payload": {}
}
```

### Per-session summary

Path:

`~/.codex/telemetry/runs/<session-id>.json`

The summary is derived during the run and written on session stop. Raw events remain the durable append-only source if summary writing fails or the process exits unexpectedly.

## Event Model

Schema version starts at `1`.

### Event types in this slice

- `session_started`
- `turn_started`
- `turn_completed`
- `turn_aborted`
- `turn_errored`
- `token_usage_checkpoint`
- `tool_call_started`
- `tool_call_finished`
- `approval_recorded`
- `session_completed`

### Session start payload

Capture when enabled and available:

- telemetry schema version
- Codex CLI version
- invocation mode:
  - interactive
  - exec
  - app
  - mcp-server
- timestamp start
- session id / conversation id / rollout id if available
- session source
- working directory
- session rollout file path if available
- git repo root when available
- sanitized git remote identity or stable hash
- git branch
- git commit SHA before run
- dirty working tree boolean before run
- selected model
- selected reasoning effort
- approval policy
- sandbox mode
- active profile
- effective config sources without secrets
- whether developer instructions, AGENTS.md, and project instructions were loaded if detectable
- prompt metadata:
  - prompt byte length
  - prompt token estimate when cheaply available
  - prompt SHA-256 when `hash_prompts = true`
  - full prompt only when `log_user_prompt = true`
- relevant CLI behavior args when available
- resumed/forked ancestry when available

### Turn payloads

Capture:

- turn id
- timestamps
- turn outcome:
  - completed
  - aborted
  - errored
- turn prompt metadata with the same privacy rules as session start
- turn token deltas from existing token usage flow

### Token usage payload

Capture from existing `TokenUsageInfo` / `token_count` flow:

- input tokens
- cached input tokens
- output tokens
- reasoning output tokens
- total tokens
- model context window if available
- rate limit snapshot when available
- `last_token_usage`
- `total_token_usage`

This is a mirror of existing runtime facts, not a new source of truth.

### Tool payloads

Capture:

- tool name
- tool call id
- start/end timestamps
- duration
- success/failure/cancelled
- shell command classification when known
- approval requirement / escalation flags when known

By default, do not capture:

- tool output
- command output
- file contents

### Approval payloads

Capture:

- approval target kind
- approval id when available
- request timestamp
- granted / denied / bypassed
- whether guardian or user path handled the decision when available

### Session completed payload

Capture:

- timestamp end
- duration
- final status:
  - completed
  - aborted
  - errored
  - unknown
- git commit SHA after run when enabled
- dirty working tree boolean after run when enabled
- changed file paths when safe and cheap
- changed file counts by extension when safe and cheap
- insertion/deletion summary when cheap
- approvals requested/granted/denied counts
- tool call counts and failures
- final total token usage
- pointers to rollout/session files

## Summary Schema

The per-session summary should be sufficient for later reporting without rereading every raw event for common queries.

Fields:

- `schema_version`
- `session_id`
- `started_at`
- `ended_at`
- `duration_ms`
- `invocation_mode`
- `session_source`
- `model`
- `reasoning_effort`
- `approval_policy`
- `sandbox_mode`
- `cwd`
- `repo_root`
- `git`
- `prompt_metadata`
- `usage_totals`
- `turn_counts`
- `tool_summary`
- `approval_summary`
- `error_summary`
- `changed_files_summary`
- `rollout_path`
- `raw_event_path`
- `resumed_from`
- `forked_from`

The summary should contain prompt metadata and hashes, not prompt text, unless prompt logging is explicitly enabled.

## Privacy and Redaction Rules

### Default persisted content

Persist only metadata, identifiers, hashes, counters, timings, booleans, and sanitized configuration details.

### Explicitly excluded by default

- full prompt text
- assistant text
- tool output
- shell output
- diffs
- file contents
- raw AGENTS or instruction text
- secrets from config or environment

### Config snapshot behavior

Config snapshot capture should record:

- profile name
- config file provenance
- effective toggles relevant to telemetry interpretation

It should not record:

- auth tokens
- API keys
- raw environment variables
- secret-bearing string values

## Runtime Integration Points

### Session initialization

At session creation:

- create the local telemetry run context when enabled
- resolve local telemetry paths
- emit `session_started`

This likely requires a small `core` bridge because not all startup facts are currently exposed through extension inputs.

### Turn lifecycle

Use existing turn lifecycle hooks to emit:

- `turn_started`
- `turn_completed`
- `turn_aborted`
- `turn_errored`

### Token usage

Use the existing token usage contributor path so local telemetry mirrors the same usage facts already sent through `token_count`.

### Tool lifecycle

Use tool lifecycle hooks to emit tool start and finish events.

### Session shutdown

On thread/session stop:

- emit `session_completed`
- write per-session summary if enabled

If summary writing fails, the run still succeeds and raw events remain intact.

## Failure Handling

### Disabled path

When local telemetry is disabled:

- instantiate a no-op writer
- avoid path creation
- avoid background work

### Write failures

On directory creation, append, or summary write failure:

- log a warning or debug event
- do not fail the Codex run
- continue best-effort capture for future events if possible

### Partial facts

If some metadata is unavailable:

- write `null` or omit only inside payload-specific optional fields
- preserve the top-level event schema

## Testing Strategy

### Config

- parse defaults for local telemetry config
- verify explicit privacy overrides
- verify resolved directory behavior

### Writer behavior

- disabled writer produces no files
- enabled writer creates expected raw event path
- enabled writer writes summary on completion
- writer failures do not propagate as run failures

### Privacy

- default config does not store prompt text
- prompt hash is written when hashing is enabled
- prompt text is written only when `log_user_prompt = true`
- tool output is excluded by default

### Usage reuse

- token usage capture mirrors existing `token_count` / `TokenUsageInfo` facts
- summary totals reflect accumulated usage checkpoints

### Session modes

- `codex exec --json "small test prompt"` creates raw event and summary files
- interactive session creates raw event and summary files

### Regression

- existing rollout persistence under `~/.codex/sessions` remains unchanged
- existing OTel behavior remains unchanged

## Compatibility and Risk Notes

### OTel compatibility

This slice must not change:

- OTel config semantics
- OTel exporters
- existing `SessionTelemetry` event routing

Local telemetry is additive only.

### Rollout compatibility

This slice must not:

- change rollout file locations
- change rollout schema
- change resume/fork logic

### Core size and churn

Avoid large edits in high-churn `core` modules. Prefer:

- new telemetry crates
- new extension installation
- narrow metadata plumbing in `core`

## Commit Breakdown

Recommended commits on one feature branch:

1. Add local telemetry config types and schema updates
2. Add `codex-local-telemetry` event, summary, and writer crate
3. Add `codex-ext-local-telemetry` extension crate
4. Wire extension into session startup and shutdown in `core`
5. Add capture tests
6. Add documentation and sample config updates

## Open Decisions Chosen For This Slice

- Local telemetry is enabled by default
- Local telemetry is stored under `~/.codex/telemetry`
- Raw storage format is append-only JSONL
- Per-session summary is written in JSON
- Daily rollups and CLI analysis are deferred
- OTel remains unchanged and separate

## Acceptance For This Slice

- Enabling local telemetry creates a raw event JSONL file and run summary for `codex exec`
- Enabling local telemetry creates a raw event JSONL file and run summary for interactive sessions
- Privacy defaults do not persist prompt text, assistant text, tool output, shell output, diffs, or file contents
- Existing rollout behavior remains intact
- Existing OTel behavior remains intact
- Telemetry write failures do not interrupt Codex operation
