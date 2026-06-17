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

---

## Task 4 Fix Follow-up: review findings addressed

### Findings fixed

1. **Public API trimmed to the task boundary**
   - `codex_local_telemetry_extension` now exposes only `install`.
   - `LocalTelemetryExtension`, `LocalTelemetryRunState`, `SessionTelemetryBootstrap`,
     `SessionStopMetadata`, and `LocalTelemetryWriterHandle` are no longer public crate API.
   - `install` now constructs the extension internally, so downstream callers do not need the
     extension type at all.

2. **Automated lifecycle coverage added**
   - Added dedicated crate-local tests in `codex-rs/ext/local-telemetry/src/extension_tests.rs`.
   - Coverage now exercises:
     - thread start and stop
     - turn start and stop
     - token usage checkpoints
     - tool start and finish
     - aborted and errored turn paths
     - summary flush on thread stop

### Verification

Ran exactly:

```bash
just test -p codex-local-telemetry-extension
```

Result:

- passed
- `3 tests run: 3 passed, 0 skipped`

Ran again after removing unused test imports:

```bash
just test -p codex-local-telemetry-extension
```

Result:

- passed
- `3 tests run: 3 passed, 0 skipped`

Ran formatting per repo instruction:

```bash
just fmt
```

Result:

- partially succeeded
- Rust formatter completed
- overall command failed because this environment does not have `dotslash` for the Bazel/Starlark
  formatter, and `uv` could not open `/Users/russell/.cache/uv`

Ran crate-local Rust formatting explicitly:

```bash
cargo fmt --package codex-local-telemetry-extension
```

Result:

- passed
- emitted the standard nightly-only `imports_granularity = Item` warning, but formatted the crate

### Self-review

- No remaining task-boundary API leak is visible from `src/lib.rs`; the public entry point is now
  only `install`.
- The added tests go through the real extension registry contributor interfaces instead of testing
  helper functions in isolation.
- I did not wire anything into `codex-core`, and I did not touch files outside the allowed Task 4
  ownership surface plus the required report file.

### Fix commit

- `3011dbae0 telemetry: tighten local extension API`

## Task 4 Follow-up: host seeding helpers for Task 5 compatibility

### Why this follow-up was necessary

The narrowed Task 4 API removed the public attachment types that later host wiring needs to seed
session metadata into `ExtensionData`. Without a public host entry point, `codex-core` would have
no way to provide the writer handle, startup bootstrap, or stop metadata that this extension reads
at runtime.

### What changed

- kept the extension-owned run state private
- kept `install` as the primary extension registration entry point
- added a public `SessionTelemetryBootstrap` value type
- added `initialize_session_data(...)` so the host can seed the writer handle and startup facts
- added `update_session_stop_metadata(...)` so the host can publish shutdown-only facts before
  thread stop hooks run

This keeps the runtime state encapsulated while restoring a narrow, explicit integration surface
for Task 5.

### Verification

Ran:

```bash
cargo fmt --package codex-local-telemetry-extension
just test -p codex-local-telemetry-extension
```

Result:

- formatting passed for the extension crate
- tests passed: `3 tests run: 3 passed, 0 skipped`
