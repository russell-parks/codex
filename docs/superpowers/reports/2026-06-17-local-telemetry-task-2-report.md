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

## Fix Report: 2026-06-17 review follow-up

### Review findings addressed

1. `JsonlTelemetryWriter::append_event` now serializes concurrent appends with a shared `Arc<tokio::sync::Mutex<()>>`, so cloned writers used behind `Arc<dyn LocalTelemetryWriter>` cannot interleave JSONL writes within this process.
2. `SessionSummary` now exposes the design-required public fields and supporting storage structs needed by later tasks:
   - `session_source`
   - `git`
   - `prompt_metadata`
   - `turn_counts`
   - `changed_files_summary`
   - `resumed_from`
   - `forked_from`

### Files changed for the fix

- `codex-rs/local-telemetry/Cargo.toml`
- `codex-rs/local-telemetry/src/lib.rs`
- `codex-rs/local-telemetry/src/summary.rs`
- `codex-rs/local-telemetry/src/writer.rs`

### Root cause

- The original writer opened the JSONL file independently per append with no shared synchronization, so overlapping async callers could write to the same file concurrently.
- The original summary model stopped short of the binding design and omitted several fields that later telemetry tasks are expected to populate directly.

### Exact verification commands and results

1. Repo-required formatting attempt:

```bash
just --justfile ../justfile fmt
```

Result:

- failed for unrelated local-environment reasons outside this crate:
  - missing `dotslash` for Bazel/Starlark formatting
  - unwritable `~/.cache/uv` for Python formatting
- Rust formatting did run before the overall `just fmt` failure

2. Crate-local Rust formatting:

```bash
cargo fmt --package codex-local-telemetry --manifest-path codex-rs/Cargo.toml --all -- codex-rs/local-telemetry/src/lib.rs codex-rs/local-telemetry/src/summary.rs codex-rs/local-telemetry/src/writer.rs
```

Result:

- succeeded
- emitted only stable-channel warnings about `imports_granularity = Item`

3. Repo-standard crate test entrypoint:

```bash
just test -p codex-local-telemetry
```

Result:

- failed before building due sandboxed Cargo state outside the crate logic:
  - Cargo could not create directories under `~/.cargo`
  - the workspace dependency resolution path touched the patched `tungstenite` git source

4. Narrow crate compilation with a temporary cargo home:

```bash
CARGO_HOME=/private/tmp/codex-cargo-home cargo check --manifest-path codex-rs/local-telemetry/Cargo.toml --offline
```

Result:

- succeeded
- finished `dev` profile for `codex-local-telemetry`

5. Narrow crate nextest invocation with no in-repo tests:

```bash
CARGO_HOME=/private/tmp/codex-cargo-home cargo nextest run --manifest-path codex-rs/local-telemetry/Cargo.toml --no-tests pass
```

Result:

- succeeded
- compiled `codex-local-telemetry`
- ran zero tests, which is expected because Task 3 owns the persistent test module

6. Focused executable verification harness for the two review fixes:

```bash
CARGO_HOME=/private/tmp/codex-cargo-home cargo test
```

Working directory:

```text
/private/tmp/local-telemetry-verify
```

Harness coverage:

- `session_summary_exposes_required_binding_fields`
- `concurrent_appends_produce_parseable_jsonl`

Result:

- succeeded
- 2 tests passed, 0 failed

### Self-review

#### Findings

No new correctness issues found in the final Task 2 fix diff.

#### Specific checks

- The crate remains storage-only and still does not add OTel or extension logic.
- The new append lock is shared across `JsonlTelemetryWriter` clones, which is the case needed for later `Arc<dyn LocalTelemetryWriter>` use.
- The added summary structs are exported publicly so later tasks can populate them directly instead of reshaping the API.
- The fix stayed inside the Task 2 ownership files.

### Remaining concerns

- The repository-wide `just fmt` and `just test -p codex-local-telemetry` entrypoints are still affected by local sandbox/tooling constraints unrelated to this crate.
- The focused verification harness lives under `/private/tmp` and is not a persistent repo test because Task 3 owns the durable test module for this crate.
