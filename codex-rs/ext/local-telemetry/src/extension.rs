use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TokenUsageContributor;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::ConfigSnapshotSummary;
use codex_local_telemetry::GitSummary;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::NoopTelemetryWriter;
use codex_local_telemetry::PromptMetadataSummary;
use codex_local_telemetry::SessionSummary;
use codex_local_telemetry::TELEMETRY_SCHEMA_VERSION;
use codex_local_telemetry::TelemetryEvent;
use codex_local_telemetry::TelemetryEventType;
use codex_local_telemetry::maybe_hash_prompt;
use codex_local_telemetry::maybe_store_prompt;
use serde_json::json;
use tokio::sync::Mutex;

use crate::state::LocalTelemetryRunState;
use crate::state::LocalTelemetryWriterHandle;
use crate::state::PromptCaptureState;
use crate::state::SessionStopMetadata;

#[derive(Debug, Clone)]
pub struct SessionTelemetryBootstrap {
    pub invocation_mode: String,
    pub cwd: String,
    pub rollout_path: Option<String>,
    pub repo_root: Option<String>,
    pub git: Option<GitSummary>,
    pub resumed_from: Option<String>,
    pub forked_from: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub active_profile: Option<String>,
    pub config_snapshot: Option<ConfigSnapshotSummary>,
    pub log_user_prompt: bool,
    pub hash_prompts: bool,
    pub write_run_summary: bool,
    pub capture_session: bool,
    pub capture_turns: bool,
    pub capture_usage: bool,
    pub capture_tool_calls: bool,
    pub capture_approvals: bool,
    pub capture_errors: bool,
}

#[derive(Debug, Default)]
struct LocalTelemetryExtension;

impl LocalTelemetryExtension {
    fn new() -> Self {
        Self
    }
}

impl<C> ThreadLifecycleContributor<C> for LocalTelemetryExtension
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let started_at = Instant::now();
            let started_at_rfc3339 = Utc::now().to_rfc3339();
            let session_id = input.thread_store.level_id().to_string();
            let writer_handle = input.session_store.get::<LocalTelemetryWriterHandle>();
            let writer: Arc<dyn LocalTelemetryWriter> = writer_handle
                .as_ref()
                .map(|handle| Arc::clone(&handle.writer))
                .unwrap_or_else(|| Arc::new(NoopTelemetryWriter));
            let bootstrap = input.session_store.get::<SessionTelemetryBootstrap>();
            let summary = SessionSummary {
                schema_version: TELEMETRY_SCHEMA_VERSION,
                session_id: session_id.clone(),
                started_at: started_at_rfc3339.clone(),
                ended_at: None,
                duration_ms: None,
                invocation_mode: bootstrap
                    .as_ref()
                    .map(|value| value.invocation_mode.clone())
                    .unwrap_or_else(|| input.session_source.to_string()),
                session_source: input.session_source.to_string(),
                model: bootstrap.as_ref().map(|value| value.model.clone()),
                reasoning_effort: bootstrap
                    .as_ref()
                    .and_then(|value| value.reasoning_effort.clone()),
                approval_policy: bootstrap
                    .as_ref()
                    .map(|value| value.approval_policy.clone()),
                sandbox_mode: bootstrap.as_ref().map(|value| value.sandbox_mode.clone()),
                active_profile: bootstrap
                    .as_ref()
                    .and_then(|value| value.active_profile.clone()),
                cwd: bootstrap.as_ref().map(|value| value.cwd.clone()),
                repo_root: bootstrap.as_ref().and_then(|value| value.repo_root.clone()),
                git: bootstrap.as_ref().and_then(|value| value.git.clone()),
                config_snapshot: bootstrap
                    .as_ref()
                    .and_then(|value| value.config_snapshot.clone()),
                prompt_metadata: Default::default(),
                raw_event_path: writer_handle
                    .as_ref()
                    .map(|handle| handle.raw_event_path.clone())
                    .unwrap_or_default(),
                rollout_path: bootstrap
                    .as_ref()
                    .and_then(|value| value.rollout_path.clone()),
                usage_totals: Default::default(),
                turn_counts: Default::default(),
                tool_summary: Default::default(),
                approval_summary: Default::default(),
                error_summary: Default::default(),
                changed_files_summary: Default::default(),
                resumed_from: bootstrap
                    .as_ref()
                    .and_then(|value| value.resumed_from.clone()),
                forked_from: bootstrap
                    .as_ref()
                    .and_then(|value| value.forked_from.clone()),
            };
            let run_state = LocalTelemetryRunState {
                session_id: session_id.clone(),
                started_at,
                started_at_rfc3339: started_at_rfc3339.clone(),
                writer,
                write_run_summary: bootstrap
                    .as_ref()
                    .map(|value| value.write_run_summary)
                    .unwrap_or(true),
                capture_session: bootstrap
                    .as_ref()
                    .map(|value| value.capture_session)
                    .unwrap_or(true),
                capture_turns: bootstrap
                    .as_ref()
                    .map(|value| value.capture_turns)
                    .unwrap_or(true),
                capture_usage: bootstrap
                    .as_ref()
                    .map(|value| value.capture_usage)
                    .unwrap_or(true),
                capture_tool_calls: bootstrap
                    .as_ref()
                    .map(|value| value.capture_tool_calls)
                    .unwrap_or(true),
                capture_approvals: bootstrap
                    .as_ref()
                    .map(|value| value.capture_approvals)
                    .unwrap_or(true),
                capture_errors: bootstrap
                    .as_ref()
                    .map(|value| value.capture_errors)
                    .unwrap_or(true),
                summary: Arc::new(Mutex::new(summary)),
            };
            input.thread_store.insert(run_state);

            if let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() {
                if !run_state.capture_session {
                    return;
                }
                append_event(
                    run_state.as_ref(),
                    TelemetryEventType::SessionStarted,
                    None,
                    json!({
                        "started_at": started_at_rfc3339,
                        "session_source": input.session_source.to_string(),
                        "invocation_mode": bootstrap
                            .as_ref()
                            .map(|value| value.invocation_mode.clone())
                            .unwrap_or_else(|| input.session_source.to_string()),
                        "cwd": bootstrap.as_ref().map(|value| value.cwd.clone()),
                        "model": bootstrap.as_ref().map(|value| value.model.clone()),
                        "reasoning_effort": bootstrap
                            .as_ref()
                            .and_then(|value| value.reasoning_effort.clone()),
                        "approval_policy": bootstrap
                            .as_ref()
                            .map(|value| value.approval_policy.clone()),
                        "sandbox_mode": bootstrap
                            .as_ref()
                            .map(|value| value.sandbox_mode.clone()),
                        "repo_root": bootstrap
                            .as_ref()
                            .and_then(|value| value.repo_root.clone()),
                        "git": bootstrap.as_ref().and_then(|value| value.git.clone()),
                        "resumed_from": bootstrap
                            .as_ref()
                            .and_then(|value| value.resumed_from.clone()),
                        "forked_from": bootstrap
                            .as_ref()
                            .and_then(|value| value.forked_from.clone()),
                        "active_profile": bootstrap
                            .as_ref()
                            .and_then(|value| value.active_profile.clone()),
                        "config_snapshot": bootstrap
                            .as_ref()
                            .and_then(|value| value.config_snapshot.clone()),
                        "log_user_prompt": bootstrap
                            .as_ref()
                            .map(|value| value.log_user_prompt),
                        "hash_prompts": bootstrap
                            .as_ref()
                            .map(|value| value.hash_prompts),
                        "rollout_path": bootstrap
                            .as_ref()
                            .and_then(|value| value.rollout_path.clone()),
                    }),
                )
                .await;
            }
        })
    }

    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };

            let ended_at = Utc::now().to_rfc3339();
            let duration_ms =
                u64::try_from(run_state.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);

            if let Some(stop_metadata) = input.session_store.get::<SessionStopMetadata>() {
                let mut summary = run_state.summary.lock().await;
                summary.rollout_path = stop_metadata.rollout_path.clone();
                if let Some(git) = &stop_metadata.git {
                    summary
                        .git
                        .get_or_insert_with(Default::default)
                        .commit_sha_after = git.commit_sha_after.clone();
                    summary.git.get_or_insert_with(Default::default).dirty_after = git.dirty_after;
                }
                if let Some(changed_files_summary) = &stop_metadata.changed_files_summary {
                    summary.changed_files_summary = changed_files_summary.clone();
                }
            }

            let payload = {
                let mut summary = run_state.summary.lock().await;
                summary.ended_at = Some(ended_at.clone());
                summary.duration_ms = Some(duration_ms);
                json!({
                    "ended_at": ended_at,
                    "duration_ms": duration_ms,
                    "usage_totals": summary.usage_totals,
                    "turn_counts": summary.turn_counts,
                    "tool_summary": summary.tool_summary,
                    "error_summary": summary.error_summary,
                    "rollout_path": summary.rollout_path,
                    "changed_files_summary": summary.changed_files_summary,
                })
            };

            if run_state.capture_session {
                append_event(
                    run_state.as_ref(),
                    TelemetryEventType::SessionCompleted,
                    None,
                    payload,
                )
                .await;
            }

            if run_state.write_run_summary {
                let summary = run_state.summary.lock().await.clone();
                if let Err(err) = run_state.writer.write_summary(&summary).await {
                    tracing::warn!(
                        "local telemetry summary write failed for {}: {err}",
                        run_state.session_id
                    );
                }
            }
        })
    }
}

impl TurnLifecycleContributor for LocalTelemetryExtension {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_turns {
                return;
            }

            {
                let mut summary = run_state.summary.lock().await;
                summary.turn_counts.started += 1;
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::TurnStarted,
                Some(input.turn_id),
                json!({
                    "turn_id": input.turn_id,
                }),
            )
            .await;
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_turns {
                return;
            }
            let turn_id = input.turn_store.level_id();
            let prompt_metadata = take_prompt_metadata(input.session_store, turn_id);

            {
                let mut summary = run_state.summary.lock().await;
                summary.turn_counts.completed += 1;
                if let Some(prompt_metadata) = &prompt_metadata {
                    summary.prompt_metadata = prompt_metadata.clone();
                }
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::TurnCompleted,
                Some(turn_id),
                json!({
                    "turn_id": turn_id,
                    "prompt_metadata": prompt_metadata,
                }),
            )
            .await;
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_turns {
                return;
            }
            let turn_id = input.turn_store.level_id();
            let prompt_metadata = take_prompt_metadata(input.session_store, turn_id);

            {
                let mut summary = run_state.summary.lock().await;
                summary.turn_counts.aborted += 1;
                if let Some(prompt_metadata) = &prompt_metadata {
                    summary.prompt_metadata = prompt_metadata.clone();
                }
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::TurnAborted,
                Some(turn_id),
                json!({
                    "turn_id": turn_id,
                    "reason": format!("{:?}", input.reason),
                    "prompt_metadata": prompt_metadata,
                }),
            )
            .await;
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_turns {
                return;
            }
            let error_text = format!("{:?}", input.error);
            let prompt_metadata = take_prompt_metadata(input.session_store, input.turn_id);

            {
                let mut summary = run_state.summary.lock().await;
                summary.turn_counts.errored += 1;
                if run_state.capture_errors {
                    summary.error_summary.error_count += 1;
                    summary.error_summary.last_error = Some(error_text.clone());
                }
                if let Some(prompt_metadata) = &prompt_metadata {
                    summary.prompt_metadata = prompt_metadata.clone();
                }
            }

            if run_state.capture_errors {
                append_event(
                    run_state.as_ref(),
                    TelemetryEventType::TurnErrored,
                    Some(input.turn_id),
                    json!({
                        "turn_id": input.turn_id,
                        "error": error_text,
                        "prompt_metadata": prompt_metadata,
                    }),
                )
                .await;
            }
        })
    }
}

impl TokenUsageContributor for LocalTelemetryExtension {
    fn on_token_usage<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        token_usage: &'a codex_protocol::protocol::TokenUsageInfo,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(run_state) = thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_usage {
                return;
            }

            {
                let mut summary = run_state.summary.lock().await;
                summary.usage_totals.input_tokens =
                    u64::try_from(token_usage.total_token_usage.input_tokens).unwrap_or_default();
                summary.usage_totals.output_tokens =
                    u64::try_from(token_usage.total_token_usage.output_tokens).unwrap_or_default();
                summary.usage_totals.reasoning_tokens =
                    u64::try_from(token_usage.total_token_usage.reasoning_output_tokens)
                        .unwrap_or_default();
                summary.usage_totals.cached_input_tokens =
                    u64::try_from(token_usage.total_token_usage.cached_input_tokens)
                        .unwrap_or_default();
                summary.usage_totals.total_tokens =
                    u64::try_from(token_usage.total_token_usage.total_tokens).unwrap_or_default();
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::TokenUsageCheckpoint,
                Some(turn_store.level_id()),
                json!({
                    "last_token_usage": token_usage.last_token_usage,
                    "total_token_usage": token_usage.total_token_usage,
                    "model_context_window": token_usage.model_context_window,
                }),
            )
            .await;
        })
    }
}

impl ToolLifecycleContributor for LocalTelemetryExtension {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_tool_calls {
                return;
            }

            {
                let mut summary = run_state.summary.lock().await;
                summary.tool_summary.total_calls += 1;
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::ToolCallStarted,
                Some(input.turn_id),
                json!({
                    "turn_id": input.turn_id,
                    "call_id": input.call_id,
                    "tool_name": input.tool_name.to_string(),
                    "source": format!("{:?}", input.source),
                }),
            )
            .await;
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            let Some(run_state) = input.thread_store.get::<LocalTelemetryRunState>() else {
                return;
            };
            if !run_state.capture_tool_calls {
                return;
            }

            {
                let mut summary = run_state.summary.lock().await;
                match input.outcome {
                    ToolCallOutcome::Completed { success } => {
                        if success {
                            summary.tool_summary.success_count += 1;
                        } else {
                            summary.tool_summary.failure_count += 1;
                        }
                    }
                    ToolCallOutcome::Blocked
                    | ToolCallOutcome::Failed {
                        handler_executed: _,
                    }
                    | ToolCallOutcome::Aborted => {
                        summary.tool_summary.failure_count += 1;
                    }
                }
            }

            append_event(
                run_state.as_ref(),
                TelemetryEventType::ToolCallFinished,
                Some(input.turn_id),
                json!({
                    "turn_id": input.turn_id,
                    "call_id": input.call_id,
                    "tool_name": input.tool_name.to_string(),
                    "source": format!("{:?}", input.source),
                    "outcome": tool_call_outcome_payload(input.outcome),
                }),
            )
            .await;
        })
    }
}

pub fn install<C>(registry: &mut ExtensionRegistryBuilder<C>)
where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(LocalTelemetryExtension::new());
    registry.thread_lifecycle_contributor(extension.clone());
    registry.turn_lifecycle_contributor(extension.clone());
    registry.token_usage_contributor(extension.clone());
    registry.tool_lifecycle_contributor(extension);
}

pub fn initialize_session_data(
    session_store: &ExtensionData,
    writer: Arc<dyn LocalTelemetryWriter>,
    raw_event_path: String,
    bootstrap: SessionTelemetryBootstrap,
) {
    session_store.insert(LocalTelemetryWriterHandle {
        raw_event_path,
        writer,
    });
    session_store.insert(bootstrap);
}

pub fn update_session_stop_metadata(session_store: &ExtensionData, rollout_path: Option<String>) {
    update_session_stop_metadata_with_details(session_store, rollout_path, None, None);
}

pub fn update_session_stop_metadata_with_git(
    session_store: &ExtensionData,
    rollout_path: Option<String>,
    git: Option<GitSummary>,
) {
    update_session_stop_metadata_with_details(session_store, rollout_path, git, None);
}

pub fn update_session_stop_metadata_with_details(
    session_store: &ExtensionData,
    rollout_path: Option<String>,
    git: Option<GitSummary>,
    changed_files_summary: Option<ChangedFilesSummary>,
) {
    session_store.insert(SessionStopMetadata {
        rollout_path,
        git,
        changed_files_summary,
    });
}

pub fn record_approval_requested(
    thread_store: &ExtensionData,
    turn_id: &str,
    approval_id: &str,
    approval_kind: &str,
) {
    let Some(run_state) = thread_store.get::<LocalTelemetryRunState>() else {
        return;
    };
    if !run_state.capture_approvals {
        return;
    }

    let run_state = run_state.clone();
    let turn_id = turn_id.to_string();
    let approval_id = approval_id.to_string();
    let approval_kind = approval_kind.to_string();
    tokio::spawn(async move {
        {
            let mut summary = run_state.summary.lock().await;
            summary.approval_summary.total_requests += 1;
        }
        append_event(
            run_state.as_ref(),
            TelemetryEventType::ApprovalRecorded,
            Some(turn_id.as_str()),
            json!({
                "turn_id": turn_id,
                "approval_id": approval_id,
                "approval_kind": approval_kind,
                "phase": "requested",
            }),
        )
        .await;
    });
}

pub fn record_approval_resolved(
    thread_store: &ExtensionData,
    turn_id: &str,
    approval_id: &str,
    approval_kind: &str,
    approved: bool,
    decision: &str,
) {
    let Some(run_state) = thread_store.get::<LocalTelemetryRunState>() else {
        return;
    };
    if !run_state.capture_approvals {
        return;
    }

    let run_state = run_state.clone();
    let turn_id = turn_id.to_string();
    let approval_id = approval_id.to_string();
    let approval_kind = approval_kind.to_string();
    let decision = decision.to_string();
    tokio::spawn(async move {
        {
            let mut summary = run_state.summary.lock().await;
            if approved {
                summary.approval_summary.approved_count += 1;
            } else {
                summary.approval_summary.denied_count += 1;
            }
        }
        append_event(
            run_state.as_ref(),
            TelemetryEventType::ApprovalRecorded,
            Some(turn_id.as_str()),
            json!({
                "turn_id": turn_id,
                "approval_id": approval_id,
                "approval_kind": approval_kind,
                "phase": if approved { "approved" } else { "denied" },
                "decision": decision,
            }),
        )
        .await;
    });
}

pub fn record_user_prompt(session_store: &ExtensionData, turn_id: &str, prompt_text: &str) {
    let Some(bootstrap) = session_store.get::<SessionTelemetryBootstrap>() else {
        return;
    };
    if !bootstrap.capture_turns {
        return;
    }
    let prompt_capture_state = session_store.get_or_init(PromptCaptureState::default);
    let metadata = PromptMetadataSummary {
        prompt_byte_length: u64::try_from(prompt_text.len()).unwrap_or(u64::MAX),
        prompt_token_estimate: None,
        prompt_sha256: maybe_hash_prompt(bootstrap.hash_prompts, prompt_text),
        prompt_text: maybe_store_prompt(bootstrap.log_user_prompt, prompt_text),
    };
    prompt_capture_state.insert(turn_id.to_string(), metadata);
}

async fn append_event(
    run_state: &LocalTelemetryRunState,
    event_type: TelemetryEventType,
    turn_id: Option<&str>,
    payload: serde_json::Value,
) {
    let event = TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        timestamp: Utc::now().to_rfc3339(),
        session_id: run_state.session_id.clone(),
        turn_id: turn_id.map(str::to_owned),
        event_type,
        payload,
    };

    if let Err(err) = run_state.writer.append_event(&event).await {
        tracing::warn!(
            "local telemetry append failed for {}: {err}",
            run_state.session_id
        );
    }
}

fn tool_call_outcome_payload(outcome: ToolCallOutcome) -> serde_json::Value {
    match outcome {
        ToolCallOutcome::Completed { success } => json!({
            "kind": "completed",
            "success": success,
        }),
        ToolCallOutcome::Blocked => json!({
            "kind": "blocked",
        }),
        ToolCallOutcome::Failed { handler_executed } => json!({
            "kind": "failed",
            "handler_executed": handler_executed,
        }),
        ToolCallOutcome::Aborted => json!({
            "kind": "aborted",
        }),
    }
}

fn take_prompt_metadata(
    session_store: &ExtensionData,
    turn_id: &str,
) -> Option<PromptMetadataSummary> {
    session_store
        .get::<PromptCaptureState>()
        .and_then(|state| state.remove(turn_id))
}
