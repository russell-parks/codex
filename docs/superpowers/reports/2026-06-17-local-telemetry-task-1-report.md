# Task 1 Report: Local telemetry config types and resolution

## Scope completed

Implemented Task 1 as a config-only change set for local telemetry:

- Added TOML config types in `codex-rs/config/src/types.rs`
- Added effective telemetry config types in `codex-rs/config/src/types.rs`
- Added `telemetry` to `ConfigToml` in `codex-rs/config/src/config_toml.rs`
- Added a focused resolver in `codex-rs/core/src/config/telemetry.rs`
- Wired telemetry resolution into `codex-rs/core/src/config/mod.rs`
- Added the requested regression test in `codex-rs/core/src/config/config_tests.rs`

No runtime telemetry capture logic was added.

## Implementation details

### Config surface

Added:

- `TelemetryConfigToml`
- `LocalTelemetryConfigToml`
- `TelemetryConfig`
- `LocalTelemetryConfig`

The type shapes match the task brief verbatim.

### Resolution defaults

Added `codex-rs/core/src/config/telemetry.rs` with:

- local telemetry enabled by default
- default directory `~/.codex/telemetry`
- default retention `90` days
- prompt hashing enabled by default
- content logging fields disabled by default
- session/turn/usage/tool/config/error summary capture defaults enabled per brief

### Effective config wiring

Added `telemetry: Option<TelemetryConfigToml>` to `ConfigToml`, then resolved it in
`Config::load_from_base_config_with_overrides` and stored the result on `Config` as
`pub telemetry: TelemetryConfig`.

### Regression coverage

Added the requested regression test:

- `load_config_applies_local_telemetry_defaults`

This checks the default-enabled path for:

- `enabled`
- `directory`
- `hash_prompts`
- `log_user_prompt`

## Commands run

### Formatting

- `env DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/llhttp/9.3.1/lib cargo fmt --all`

Result:

- Succeeded

### Focused test attempt

Attempted repo-standard verification first:

- `just test -p codex-core load_config_applies_local_telemetry_defaults`

This environment could not use the repo `just` wrapper because Homebrew `cargo` is currently broken unless `DYLD_LIBRARY_PATH` is overridden, and that override was not preserved through the wrapper.

Ran the equivalent direct command instead:

- `env PATH=/private/tmp/codex-tools/bin:$PATH DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/llhttp/9.3.1/lib CARGO_HOME=/private/tmp/codex-cargo-home RUST_MIN_STACK=8388608 NEXTEST_PROFILE=local cargo nextest run --no-fail-fast -p codex-core load_config_applies_local_telemetry_defaults`

Result:

- Blocked by an unrelated existing workspace compile failure in `codex-thread-store`

Error:

```text
error[E0594]: cannot assign to `update.advance_recency_at`, as `update` is not declared as mutable
  --> thread-store/src/thread_metadata_sync.rs:171:13
```

### Schema regeneration attempt

Attempted equivalent schema command:

- `env PATH=/private/tmp/codex-tools/bin:$PATH DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/llhttp/9.3.1/lib CARGO_HOME=/private/tmp/codex-cargo-home cargo run -p codex-core --bin codex-write-config-schema`

Result:

- Blocked by the same unrelated `codex-thread-store` compile failure

Because of that, `codex-rs/core/config.schema.json` could not be regenerated in this environment.

## Self-review

### What looks correct

- The added config types match the brief exactly
- The resolver defaults match the brief exactly
- The config integration is minimal and isolated
- Existing OTEL behavior was not changed
- The task stays config-only

### Remaining concern

- Required verification and schema regeneration are currently blocked by an unrelated pre-existing compile error in `codex-thread-store`

## Commit

Committed as requested:

- `config: add local telemetry settings`
