# Task 2 Report

Pending implementation.
# Task 2 Report: local telemetry core crate

## Outcome

Implemented Task 2 by adding the new `codex-local-telemetry` workspace crate and registering it in the workspace manifest.

## Files changed

- `codex-rs/Cargo.toml`
- `codex-rs/local-telemetry/Cargo.toml`
- `codex-rs/local-telemetry/src/lib.rs`
- `codex-rs/local-telemetry/src/schema.rs`
- `codex-rs/local-telemetry/src/summary.rs`
- `codex-rs/local-telemetry/src/privacy.rs`
- `codex-rs/local-telemetry/src/paths.rs`
- `codex-rs/local-telemetry/src/writer.rs`

## What was implemented

### Workspace registration

- Added `local-telemetry` to `codex-rs` workspace members.
- Added the `codex-local-telemetry` workspace dependency alias.

### Crate manifest

- Created `codex-rs/local-telemetry/Cargo.toml`.
- Kept dependencies limited to the storage task scope:
  - `chrono`
  - `serde`
  - `serde_json`
  - `sha2`
  - `tokio`

No OTel dependency was added.

### Event schema

Added the exact event schema anchors from the task brief:

- `TelemetryEvent`
- `TelemetryEventType`
- `TELEMETRY_SCHEMA_VERSION`

### Summary schema

Added `SessionSummary` and supporting summary types:

- `UsageTotals`
- `ToolSummary`
- `ApprovalSummary`
- `ErrorSummary`

The public `SessionSummary` shape matches the project plan expected by later telemetry extension work, including:

- identity and timing fields
- invocation/config metadata fields
- `raw_event_path`
- `rollout_path`
- nested usage/tool/approval/error summaries

### Privacy helpers

Added the exact helpers from the brief:

- `maybe_hash_prompt`
- `maybe_store_prompt`

### Path helpers

Added the exact helpers from the brief:

- `event_file_path`
- `summary_file_path`

This uses the required partitioned date layout:

- `events/YYYY/MM/DD/<session>.jsonl`
- `runs/<session>.json`

### Writer abstraction

Added an object-safe writer trait suitable for later `Arc<dyn ...>` extension state:

- `LocalTelemetryWriter`

Added two implementations:

- `NoopTelemetryWriter`
- `JsonlTelemetryWriter`

`JsonlTelemetryWriter`:

- appends newline-delimited JSON events
- writes pretty JSON summaries
- creates parent directories on demand
- exposes path accessors for later extension/state code

## Verification

### Formatting

Repo policy required `just fmt`, and I attempted it first.

That command was blocked by unrelated local environment issues outside this task:

- missing `dotslash`
- `uv` cache permission failures under `~/.cache/uv`

To ensure the touched Rust files were still formatted, I ran:

```bash
rustfmt codex-rs/local-telemetry/src/lib.rs \
  codex-rs/local-telemetry/src/schema.rs \
  codex-rs/local-telemetry/src/summary.rs \
  codex-rs/local-telemetry/src/privacy.rs \
  codex-rs/local-telemetry/src/paths.rs \
  codex-rs/local-telemetry/src/writer.rs
```

### Tests / build verification

I attempted the repo-mandated crate-scoped test command:

```bash
just test -p codex-local-telemetry
```

This required environment repair before it could run:

- installed missing `just`
- repaired broken Homebrew `libgit2` linkage
- installed missing `cargo-nextest`
- used a temporary `CARGO_HOME` because sandboxed cargo writes under `~/.cargo` were not permitted

`just test -p codex-local-telemetry` then compiled the crate successfully, but `nextest` exited non-zero because this task intentionally adds zero tests and `nextest` treats "no tests" as failure by default.

To finish narrow verification for this crate-creation task, I ran:

```bash
CARGO_HOME=/private/tmp/codex-cargo-home \
  cargo nextest run -p codex-local-telemetry --no-tests pass
```

Result:

- crate compiled successfully
- verification completed successfully
- no tests were present yet, which is expected because Task 3 owns test coverage

## Self-review

### Findings

No correctness issues found in the final crate diff relative to Task 2 scope.

### Specific checks

- The crate stays storage-only and does not pull in OTel.
- The writer trait is object-safe and compatible with the Task 4 `Arc<dyn LocalTelemetryWriter>` requirement.
- The public API exports the event, summary, privacy, path, and writer surfaces expected by later tasks.
- No unrelated source files were changed.
- A generated `Cargo.lock` entry introduced during verification was removed so the final change stayed within the file scope for this task.

## Concerns

- `just fmt` could not complete end-to-end because of unrelated local tool/environment issues (`dotslash` missing and `uv` cache permissions). The touched Rust files were formatted with `rustfmt` instead.
- Task 2 intentionally contains no tests; Task 3 is still needed to add crate-level coverage.
