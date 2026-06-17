# Task 4 Report: local telemetry extension crate

## Scope

Implemented Task 4 within the allowed surface only:

- registered the new workspace member in `codex-rs/Cargo.toml`
- created `codex-rs/ext/local-telemetry`
- added extension-owned run state
- implemented thread, turn, token-usage, and tool lifecycle contributors
- exposed a single install entry point and the extension type

I did not wire the crate into `codex-core`.

## What changed

### Workspace registration

Added:

- `ext/local-telemetry` to `[workspace].members`
- `codex-local-telemetry-extension` to `[workspace.dependencies]`

### New crate

Created `codex-rs/ext/local-telemetry/Cargo.toml` as:

- package: `codex-local-telemetry-extension`
- library: `codex_local_telemetry_extension`
- narrow dependencies only for the extension API and telemetry event/schema types

### Public crate surface

`src/lib.rs` now:

- declares `extension` and `state`
- exports `install`
- exports `LocalTelemetryExtension`
- exports the state/bootstrap types that later core wiring can seed into extension data

### Extension-owned state

`src/state.rs` includes the required `LocalTelemetryRunState` shape from the brief:

- `session_id`
- `started_at`
- `started_at_rfc3339`
- `writer`
- `summary`

I also added small public seed types so later core tasks can pass startup/shutdown facts and a concrete writer through `ExtensionData` without reopening this crate:

- `SessionTelemetryBootstrap`
- `SessionStopMetadata`
- `LocalTelemetryWriterHandle`

Those are intentionally simple typed attachments because `ExtensionData` is type-based and Task 5 is not allowed to edit this crate.

### Lifecycle contributors

`src/extension.rs` adds `LocalTelemetryExtension` and contributor implementations for:

- `ThreadLifecycleContributor`
- `TurnLifecycleContributor`
- `TokenUsageContributor`
- `ToolLifecycleContributor`

Current behavior:

- thread start initializes a run state, seeds a default summary, and emits `session_started`
- thread stop emits `session_completed` and writes the final summary
- turn start/stop/abort/error emit the corresponding telemetry events and update summary counters
- token usage checkpoints emit `token_usage_checkpoint` and mirror totals into the summary
- tool start/finish emit tool lifecycle events and update tool counters

All event writes are best-effort and log warnings on failure.

## Verification

Ran:

```bash
cargo check -p codex-local-telemetry-extension
```

Result: passed.

I also ran:

```bash
just fmt
```

`just fmt` did not fully complete because the environment lacks `dotslash` and the sandbox cannot create the `uv` cache under `/Users/russell/.cache/uv`. The Rust formatter phase did run, and I then ran:

```bash
cargo fmt --package codex-local-telemetry-extension
```

to ensure the new crate is formatted.

## Self-review

### What I checked

- the crate stays within the allowed ownership boundary
- the install function matches the brief’s exact registration pattern
- the required run-state struct fields match the brief verbatim
- the new public seed types make Task 5 possible without additional edits here
- event emission paths compile against the current extension API and telemetry schema

### Known limitations

- no integration wiring exists yet, so real sessions still need Task 5 before this extension captures non-noop telemetry in practice
- summary contents are intentionally skeletal for now; prompt metadata, richer summary fields, and more complete shutdown facts are deferred to later tasks
- no dedicated tests were added in this task because the task scope restricted edits to the new crate files plus the workspace manifest

## Commit

Requested commit message:

- `telemetry: add local telemetry extension crate`
