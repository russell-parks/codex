use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolName;
use codex_extension_api::ToolStartInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::ConfigSnapshotSummary;
use codex_local_telemetry::ConfigSourceSummary;
use codex_local_telemetry::GitSummary;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::RuntimeSummary;
use codex_local_telemetry::SessionSummary;
use codex_local_telemetry::TELEMETRY_SCHEMA_VERSION;
use codex_local_telemetry::TelemetryEvent;
use codex_local_telemetry::TelemetryEventType;
use codex_local_telemetry::UsageTotals;
use codex_protocol::ThreadId;
use codex_protocol::account::PlanType;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::CreditsSnapshot;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use codex_protocol::protocol::RateLimitReachedType;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TokenUsageInfo;
use codex_protocol::protocol::TurnAbortReason;
use pretty_assertions::assert_eq;
use tokio::sync::Mutex;

use crate::SessionStopUpdate;
use crate::SessionTelemetryBootstrap;
use crate::initialize_session_data;
use crate::install;
use crate::record_user_prompt;
use crate::state::LocalTelemetryRunState;
use crate::state::PromptCaptureState;

#[derive(Debug, Default)]
struct RecordingTelemetryWriter {
    events: Mutex<Vec<TelemetryEvent>>,
    summaries: Mutex<Vec<SessionSummary>>,
}

impl RecordingTelemetryWriter {
    async fn events(&self) -> Vec<TelemetryEvent> {
        self.events.lock().await.clone()
    }

    async fn summaries(&self) -> Vec<SessionSummary> {
        self.summaries.lock().await.clone()
    }
}

impl LocalTelemetryWriter for RecordingTelemetryWriter {
    fn append_event<'a>(
        &'a self,
        event: &'a TelemetryEvent,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.events.lock().await.push(event.clone());
            Ok(())
        })
    }

    fn write_summary<'a>(
        &'a self,
        summary: &'a SessionSummary,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = io::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.summaries.lock().await.push(summary.clone());
            Ok(())
        })
    }
}

struct Harness {
    session_store: ExtensionData,
    thread_store: ExtensionData,
    registry: codex_extension_api::ExtensionRegistry<()>,
    writer: Arc<RecordingTelemetryWriter>,
}

impl Harness {
    async fn start() -> Self {
        let writer = Arc::new(RecordingTelemetryWriter::default());
        let session_init = ExtensionDataInit::new();
        let session_store = ExtensionData::new_with_init("session-1", session_init);
        initialize_session_data(
            &session_store,
            writer.clone(),
            "/tmp/raw-events.jsonl".to_string(),
            SessionTelemetryBootstrap {
                invocation_mode: "cli".to_string(),
                cwd: "/tmp/worktree".to_string(),
                rollout_path: Some("/tmp/original.rollout".to_string()),
                repo_root: Some("/tmp".to_string()),
                git: Some(GitSummary {
                    remote: Some("github.com/openai/codex".to_string()),
                    branch: Some("feat/local-telemetry".to_string()),
                    commit_sha_before: Some("abc123".to_string()),
                    commit_sha_after: None,
                    dirty_before: Some(false),
                    dirty_after: None,
                }),
                resumed_from: Some(ThreadId::new().to_string()),
                forked_from: Some(ThreadId::new().to_string()),
                model: "gpt-5".to_string(),
                reasoning_effort: Some("medium".to_string()),
                approval_policy: "on-failure".to_string(),
                sandbox_mode: "workspace-write".to_string(),
                active_profile: Some("safe".to_string()),
                config_snapshot: Some(ConfigSnapshotSummary {
                    config_sources: vec![
                        ConfigSourceSummary {
                            kind: "system".to_string(),
                            source: "system (/etc/codex/config.toml)".to_string(),
                            profile: None,
                        },
                        ConfigSourceSummary {
                            kind: "user".to_string(),
                            source: "user (/tmp/.codex/config.toml)".to_string(),
                            profile: Some("safe".to_string()),
                        },
                    ],
                    developer_instructions_loaded: true,
                    user_instructions_loaded: false,
                    user_instruction_source: None,
                    project_instructions_loaded: true,
                    project_instruction_sources: vec!["/tmp/AGENTS.md".to_string()],
                }),
                log_user_prompt: false,
                log_assistant_text: false,
                log_tool_output: false,
                log_diffs: false,
                hash_prompts: true,
                write_run_summary: true,
                capture_session: true,
                capture_turns: true,
                capture_usage: true,
                capture_tool_calls: true,
                capture_approvals: true,
                capture_errors: true,
            },
        );
        let thread_store = ExtensionData::new("thread-1");
        let mut builder = ExtensionRegistryBuilder::<()>::new();
        install(&mut builder);
        let registry = builder.build();

        registry.thread_lifecycle_contributors()[0]
            .on_thread_start(ThreadStartInput {
                config: &(),
                session_source: &SessionSource::Cli,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;

        Self {
            session_store,
            thread_store,
            registry,
            writer,
        }
    }

    async fn stop(&self) {
        self.registry.thread_lifecycle_contributors()[0]
            .on_thread_stop(ThreadStopInput {
                session_store: &self.session_store,
                thread_store: &self.thread_store,
            })
            .await;
    }
}

fn collaboration_mode() -> CollaborationMode {
    CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    }
}

fn token_usage_info() -> TokenUsageInfo {
    TokenUsageInfo {
        total_token_usage: TokenUsage {
            input_tokens: 10,
            cached_input_tokens: 2,
            cache_write_input_tokens: 0,
            output_tokens: 4,
            reasoning_output_tokens: 1,
            total_tokens: 15,
        },
        last_token_usage: TokenUsage {
            input_tokens: 3,
            cached_input_tokens: 1,
            cache_write_input_tokens: 0,
            output_tokens: 2,
            reasoning_output_tokens: 1,
            total_tokens: 6,
        },
        model_context_window: Some(128_000),
    }
}

fn rate_limit_snapshot() -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit_id: Some("codex".to_string()),
        limit_name: Some("Codex".to_string()),
        primary: Some(RateLimitWindow {
            used_percent: 12.5,
            window_minutes: Some(60),
            resets_at: Some(1_718_640_000),
        }),
        secondary: None,
        credits: Some(CreditsSnapshot {
            has_credits: true,
            unlimited: false,
            balance: Some("10.00".to_string()),
        }),
        individual_limit: None,
        spend_control_reached: None,
        plan_type: Some(PlanType::Plus),
        rate_limit_reached_type: Some(RateLimitReachedType::RateLimitReached),
    }
}

#[tokio::test]
async fn thread_lifecycle_writes_session_started_and_completed() {
    let harness = Harness::start().await;
    crate::update_session_stop_metadata_with_details(
        &harness.session_store,
        SessionStopUpdate {
            rollout_path: Some("/tmp/final.rollout".to_string()),
            git: Some(GitSummary {
                remote: None,
                branch: None,
                commit_sha_before: None,
                commit_sha_after: Some("def456".to_string()),
                dirty_before: None,
                dirty_after: Some(true),
            }),
            changed_files_summary: Some(ChangedFilesSummary {
                paths: vec!["README.md".to_string(), "src/main.rs".to_string()],
                counts_by_extension: [("md".to_string(), 1_u64), ("rs".to_string(), 1_u64)]
                    .into_iter()
                    .collect(),
                insertions: Some(12),
                deletions: Some(3),
            }),
            runtime_summary: Some(RuntimeSummary {
                api_request_count: 0,
                retry_count: 0,
                latest_rate_limits: None,
                bytes_read: Some(1_024),
                bytes_written: Some(512),
            }),
            final_outcome: Some("completed".to_string()),
            abort_reason: None,
        },
    );

    let run_state = harness
        .thread_store
        .get::<LocalTelemetryRunState>()
        .expect("run state should be installed at thread start");
    assert_eq!("thread-1", run_state.session_id);

    harness.stop().await;

    let events = harness.writer.events().await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        vec![
            TelemetryEventType::SessionStarted,
            TelemetryEventType::SessionCompleted,
        ]
    );
    assert_eq!(events[0].session_id, "thread-1");
    assert_eq!(events[0].payload["invocation_mode"], "cli");
    assert_eq!(events[0].payload["repo_root"], "/tmp");
    assert_eq!(
        events[0].payload["git"]["branch"],
        serde_json::Value::String("feat/local-telemetry".to_string())
    );
    assert!(events[0].payload["resumed_from"].is_string());
    assert!(events[0].payload["forked_from"].is_string());
    assert_eq!(events[0].payload["active_profile"], "safe");
    assert_eq!(
        events[0].payload["config_snapshot"]["project_instruction_sources"][0],
        "/tmp/AGENTS.md"
    );
    assert_eq!(events[1].payload["rollout_path"], "/tmp/final.rollout");
    assert_eq!(events[1].payload["changed_files_summary"]["insertions"], 12);
    assert_eq!(events[1].payload["final_outcome"], "completed");
    assert_eq!(events[1].payload["abort_reason"], serde_json::Value::Null);
    assert_eq!(
        events[1].payload["exit_status_code"],
        serde_json::Value::Null
    );
    assert_eq!(events[1].payload["runtime_summary"]["bytes_read"], 1_024);
    assert_eq!(events[1].payload["runtime_summary"]["bytes_written"], 512);

    let summaries = harness.writer.summaries().await;
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    let expected = SessionSummary {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        session_id: "thread-1".to_string(),
        started_at: summary.started_at.clone(),
        ended_at: summary.ended_at.clone(),
        duration_ms: summary.duration_ms,
        final_outcome: Some("completed".to_string()),
        abort_reason: None,
        exit_status_code: None,
        invocation_mode: "cli".to_string(),
        session_source: "cli".to_string(),
        model: Some("gpt-5".to_string()),
        reasoning_effort: Some("medium".to_string()),
        approval_policy: Some("on-failure".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        active_profile: Some("safe".to_string()),
        cwd: Some("/tmp/worktree".to_string()),
        repo_root: Some("/tmp".to_string()),
        git: Some(GitSummary {
            remote: Some("github.com/openai/codex".to_string()),
            branch: Some("feat/local-telemetry".to_string()),
            commit_sha_before: Some("abc123".to_string()),
            commit_sha_after: Some("def456".to_string()),
            dirty_before: Some(false),
            dirty_after: Some(true),
        }),
        config_snapshot: Some(ConfigSnapshotSummary {
            config_sources: vec![
                ConfigSourceSummary {
                    kind: "system".to_string(),
                    source: "system (/etc/codex/config.toml)".to_string(),
                    profile: None,
                },
                ConfigSourceSummary {
                    kind: "user".to_string(),
                    source: "user (/tmp/.codex/config.toml)".to_string(),
                    profile: Some("safe".to_string()),
                },
            ],
            developer_instructions_loaded: true,
            user_instructions_loaded: false,
            user_instruction_source: None,
            project_instructions_loaded: true,
            project_instruction_sources: vec!["/tmp/AGENTS.md".to_string()],
        }),
        prompt_metadata: Default::default(),
        raw_event_path: "/tmp/raw-events.jsonl".to_string(),
        rollout_path: Some("/tmp/final.rollout".to_string()),
        usage_totals: Default::default(),
        turn_counts: Default::default(),
        tool_summary: Default::default(),
        runtime_summary: RuntimeSummary {
            api_request_count: 0,
            retry_count: 0,
            latest_rate_limits: None,
            bytes_read: Some(1_024),
            bytes_written: Some(512),
        },
        task_types: Vec::new(),
        approval_summary: Default::default(),
        error_summary: Default::default(),
        changed_files_summary: ChangedFilesSummary {
            paths: vec!["README.md".to_string(), "src/main.rs".to_string()],
            counts_by_extension: [("md".to_string(), 1_u64), ("rs".to_string(), 1_u64)]
                .into_iter()
                .collect(),
            insertions: Some(12),
            deletions: Some(3),
        },
        resumed_from: summary.resumed_from.clone(),
        forked_from: summary.forked_from.clone(),
    };
    assert_eq!(&expected, summary);
    assert!(summary.ended_at.is_some());
    assert!(summary.duration_ms.is_some());
}

#[tokio::test]
async fn lifecycle_callbacks_update_summary_and_emit_events() {
    let harness = Harness::start().await;
    let turn_store = ExtensionData::new("turn-1");
    let turn_contributor = &harness.registry.turn_lifecycle_contributors()[0];
    let token_contributor = &harness.registry.token_usage_contributors()[0];
    let tool_contributor = &harness.registry.tool_lifecycle_contributors()[0];
    let collaboration_mode = collaboration_mode();
    let token_usage = token_usage_info();
    let tool_name = ToolName::plain("exec_command");
    record_user_prompt(&harness.session_store, "turn-1", "prompt body");

    turn_contributor
        .on_turn_start(TurnStartInput {
            turn_id: "turn-1",
            collaboration_mode: &collaboration_mode,
            token_usage_at_turn_start: &TokenUsage::default(),
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
        })
        .await;
    token_contributor
        .on_token_usage(
            &harness.session_store,
            &harness.thread_store,
            &turn_store,
            &token_usage,
        )
        .await;
    tool_contributor
        .on_tool_start(ToolStartInput {
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
            turn_id: "turn-1",
            call_id: "call-1",
            tool_name: &tool_name,
            source: ToolCallSource::Direct,
        })
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    tool_contributor
        .on_tool_finish(ToolFinishInput {
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
            turn_id: "turn-1",
            call_id: "call-1",
            tool_name: &tool_name,
            source: ToolCallSource::Direct,
            outcome: ToolCallOutcome::Completed { success: true },
        })
        .await;
    crate::record_turn_profile(
        &harness.session_store,
        "turn-1",
        &codex_analytics::TurnProfile {
            before_first_sampling_ms: 10,
            sampling_ms: 20,
            compaction_ms: 0,
            between_sampling_overhead_ms: 5,
            tool_blocking_ms: 15,
            after_last_sampling_ms: 8,
            sampling_request_count: 2,
            sampling_retry_count: 1,
        },
    );
    crate::record_rate_limits(&harness.session_store, "turn-1", &rate_limit_snapshot());
    crate::record_task_type(&harness.session_store, "review");
    crate::record_task_type(&harness.session_store, "regular");
    crate::record_task_type(&harness.session_store, "review");
    tokio::task::yield_now().await;
    turn_contributor
        .on_turn_stop(TurnStopInput {
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
        })
        .await;
    harness.stop().await;

    let events = harness.writer.events().await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        vec![
            TelemetryEventType::SessionStarted,
            TelemetryEventType::TurnStarted,
            TelemetryEventType::TokenUsageCheckpoint,
            TelemetryEventType::ToolCallStarted,
            TelemetryEventType::ToolCallFinished,
            TelemetryEventType::TurnProfileRecorded,
            TelemetryEventType::RateLimitsRecorded,
            TelemetryEventType::TurnCompleted,
            TelemetryEventType::SessionCompleted,
        ]
    );
    assert_eq!(events[2].turn_id.as_deref(), Some("turn-1"));
    assert_eq!(events[4].payload["outcome"]["kind"], "completed");
    assert_eq!(events[4].payload["outcome"]["success"], true);
    assert_eq!(events[5].payload["sampling_request_count"], 2);
    assert_eq!(events[6].payload["rate_limits"]["plan_type"], "plus");
    assert_eq!(
        events[7].payload["prompt_metadata"]["prompt_byte_length"],
        11
    );
    assert_eq!(
        events[7].payload["prompt_metadata"]["prompt_text"],
        serde_json::Value::Null
    );

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.turn_counts.started, 1);
    assert_eq!(summary.turn_counts.completed, 1);
    assert_eq!(summary.tool_summary.total_calls, 1);
    assert_eq!(summary.tool_summary.success_count, 1);
    assert_eq!(summary.tool_summary.failure_count, 0);
    assert_eq!(summary.tool_summary.shell_command_count, 1);
    assert!(summary.tool_summary.total_duration_ms > 0);
    assert_eq!(summary.usage_totals.input_tokens, 10);
    assert_eq!(summary.usage_totals.cached_input_tokens, 2);
    assert_eq!(summary.usage_totals.output_tokens, 4);
    assert_eq!(summary.usage_totals.reasoning_tokens, 1);
    assert_eq!(summary.usage_totals.total_tokens, 15);
    assert_eq!(
        summary
            .usage_totals
            .last_token_usage
            .as_ref()
            .map(|value| value.total_tokens),
        Some(6)
    );
    assert_eq!(summary.usage_totals.model_context_window, Some(128_000));
    assert_eq!(summary.runtime_summary.api_request_count, 2);
    assert_eq!(summary.runtime_summary.retry_count, 1);
    assert_eq!(
        summary.task_types,
        vec!["regular".to_string(), "review".to_string()]
    );
    assert_eq!(
        summary
            .runtime_summary
            .latest_rate_limits
            .as_ref()
            .and_then(|value| value.plan_type.as_deref()),
        Some("plus")
    );
    assert_eq!(summary.prompt_metadata.prompt_byte_length, 11);
    assert_eq!(summary.prompt_metadata.prompt_text, None);
    assert!(summary.prompt_metadata.prompt_sha256.is_some());
}

#[tokio::test]
async fn aborted_and_errored_turns_update_counters() {
    let harness = Harness::start().await;
    let turn_store = ExtensionData::new("turn-2");
    let turn_contributor = &harness.registry.turn_lifecycle_contributors()[0];

    turn_contributor
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
        })
        .await;
    turn_contributor
        .on_turn_error(TurnErrorInput {
            turn_id: "turn-2",
            error: CodexErrorInfo::Other,
            session_store: &harness.session_store,
            thread_store: &harness.thread_store,
            turn_store: &turn_store,
        })
        .await;
    harness.stop().await;

    let events = harness.writer.events().await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        vec![
            TelemetryEventType::SessionStarted,
            TelemetryEventType::TurnAborted,
            TelemetryEventType::TurnErrored,
            TelemetryEventType::SessionCompleted,
        ]
    );
    assert_eq!(events[1].payload["reason"], "Interrupted");
    assert_eq!(events[2].payload["error"], "Other");

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.turn_counts.aborted, 1);
    assert_eq!(summary.turn_counts.errored, 1);
    assert_eq!(summary.error_summary.error_count, 1);
    assert_eq!(summary.error_summary.last_error.as_deref(), Some("Other"));
}

#[tokio::test]
async fn interrupted_session_stop_metadata_is_written_to_summary() {
    let harness = Harness::start().await;
    crate::update_session_stop_metadata_with_details(
        &harness.session_store,
        SessionStopUpdate {
            rollout_path: Some("/tmp/final.rollout".to_string()),
            git: None,
            changed_files_summary: None,
            runtime_summary: None,
            final_outcome: Some("interrupted".to_string()),
            abort_reason: Some("interrupted".to_string()),
        },
    );

    harness.stop().await;

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.final_outcome.as_deref(), Some("interrupted"));
    assert_eq!(summary.abort_reason.as_deref(), Some("interrupted"));
    assert_eq!(summary.exit_status_code, None);

    let events = harness.writer.events().await;
    let event = events
        .last()
        .expect("session completed event should be written");
    assert_eq!(event.event_type, TelemetryEventType::SessionCompleted);
    assert_eq!(event.payload["final_outcome"], "interrupted");
    assert_eq!(event.payload["abort_reason"], "interrupted");
}

#[tokio::test]
async fn prompt_text_is_stored_only_when_enabled() {
    let writer = Arc::new(RecordingTelemetryWriter::default());
    let session_store = ExtensionData::new("session-2");
    initialize_session_data(
        &session_store,
        writer.clone(),
        "/tmp/raw-events.jsonl".to_string(),
        SessionTelemetryBootstrap {
            invocation_mode: "cli".to_string(),
            cwd: "/tmp/worktree".to_string(),
            rollout_path: None,
            repo_root: None,
            git: None,
            resumed_from: None,
            forked_from: None,
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            approval_policy: "never".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            active_profile: None,
            config_snapshot: None,
            log_user_prompt: true,
            log_assistant_text: false,
            log_tool_output: false,
            log_diffs: false,
            hash_prompts: false,
            write_run_summary: false,
            capture_session: true,
            capture_turns: true,
            capture_usage: true,
            capture_tool_calls: true,
            capture_approvals: true,
            capture_errors: true,
        },
    );
    record_user_prompt(&session_store, "turn-2", "visible prompt");

    let turn_store = ExtensionData::new("turn-2");
    let thread_store = ExtensionData::new("thread-2");
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install(&mut builder);
    let registry = builder.build();
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: false,
            environments: &[],
            mcp_resource_client: None,
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;
    registry.turn_lifecycle_contributors()[0]
        .on_turn_stop(TurnStopInput {
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &turn_store,
        })
        .await;

    let run_state = thread_store
        .get::<LocalTelemetryRunState>()
        .expect("run state should be present");
    let summary = run_state.summary.lock().await.clone();
    assert_eq!(
        summary.prompt_metadata.prompt_text.as_deref(),
        Some("visible prompt")
    );
    assert_eq!(summary.prompt_metadata.prompt_sha256, None);
}

#[test]
fn record_user_prompt_is_ignored_without_bootstrap() {
    let session_store = ExtensionData::new("session-without-bootstrap");

    record_user_prompt(&session_store, "turn-1", "prompt body");

    assert!(session_store.get::<PromptCaptureState>().is_none());
}

#[tokio::test]
async fn capture_flags_disable_usage_tool_and_error_events() {
    let writer = Arc::new(RecordingTelemetryWriter::default());
    let session_store = ExtensionData::new("session-3");
    initialize_session_data(
        &session_store,
        writer.clone(),
        "/tmp/raw-events.jsonl".to_string(),
        SessionTelemetryBootstrap {
            invocation_mode: "cli".to_string(),
            cwd: "/tmp/worktree".to_string(),
            rollout_path: None,
            repo_root: None,
            git: None,
            resumed_from: None,
            forked_from: None,
            model: "gpt-5".to_string(),
            reasoning_effort: Some("medium".to_string()),
            approval_policy: "on-failure".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            active_profile: None,
            config_snapshot: None,
            log_user_prompt: false,
            log_assistant_text: false,
            log_tool_output: false,
            log_diffs: false,
            hash_prompts: true,
            write_run_summary: true,
            capture_session: true,
            capture_turns: true,
            capture_usage: false,
            capture_tool_calls: false,
            capture_approvals: false,
            capture_errors: false,
        },
    );
    let thread_store = ExtensionData::new("thread-3");
    let turn_store = ExtensionData::new("turn-3");
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install(&mut builder);
    let registry = builder.build();
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: false,
            environments: &[],
            mcp_resource_client: None,
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    registry.token_usage_contributors()[0]
        .on_token_usage(
            &session_store,
            &thread_store,
            &turn_store,
            &token_usage_info(),
        )
        .await;
    let tool_name = ToolName::plain("exec_command");
    registry.tool_lifecycle_contributors()[0]
        .on_tool_start(ToolStartInput {
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &turn_store,
            turn_id: "turn-3",
            call_id: "call-3",
            tool_name: &tool_name,
            source: ToolCallSource::Direct,
        })
        .await;
    registry.turn_lifecycle_contributors()[0]
        .on_turn_error(TurnErrorInput {
            turn_id: "turn-3",
            error: CodexErrorInfo::Other,
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &turn_store,
        })
        .await;
    registry.thread_lifecycle_contributors()[0]
        .on_thread_stop(ThreadStopInput {
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    let events = writer.events().await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        vec![
            TelemetryEventType::SessionStarted,
            TelemetryEventType::SessionCompleted,
        ]
    );

    let summaries = writer.summaries().await;
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].usage_totals, UsageTotals::default());
    assert_eq!(summaries[0].tool_summary.total_calls, 0);
    assert_eq!(summaries[0].error_summary.error_count, 0);
}

#[tokio::test]
async fn turn_item_capture_respects_privacy_defaults() {
    let harness = Harness::start().await;
    let turn_store = ExtensionData::new("turn-privacy-defaults");
    let contributor = &harness.registry.turn_item_contributors()[0];

    let mut assistant_item = TurnItem::AgentMessage(AgentMessageItem {
        id: "assistant-1".to_string(),
        content: vec![AgentMessageContent::Text {
            text: "hidden assistant text".to_string(),
        }],
        phase: None,
        memory_citation: None,
    });
    contributor
        .contribute(&harness.thread_store, &turn_store, &mut assistant_item)
        .await
        .expect("assistant contribution should succeed");

    let mut file_change_item = TurnItem::FileChange(FileChangeItem {
        id: "patch-1".to_string(),
        changes: std::collections::HashMap::from([(
            PathBuf::from("src/lib.rs"),
            FileChange::Update {
                unified_diff: "@@\n-old\n+new\n".to_string(),
                move_path: None,
            },
        )]),
        status: Some(PatchApplyStatus::Completed),
        auto_approved: Some(true),
        stdout: Some("sensitive patch stdout".to_string()),
        stderr: Some("sensitive patch stderr".to_string()),
    });
    contributor
        .contribute(&harness.thread_store, &turn_store, &mut file_change_item)
        .await
        .expect("file-change contribution should succeed");

    harness.stop().await;

    let events = harness.writer.events().await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == TelemetryEventType::TurnItemRecorded)
            .count(),
        0
    );

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.tool_summary.file_write_count, 1);
}

#[tokio::test]
async fn turn_item_capture_stores_opted_in_assistant_text_and_file_change_payloads() {
    let writer = Arc::new(RecordingTelemetryWriter::default());
    let session_store = ExtensionData::new("session-turn-items");
    initialize_session_data(
        &session_store,
        writer.clone(),
        "/tmp/raw-events.jsonl".to_string(),
        SessionTelemetryBootstrap {
            invocation_mode: "cli".to_string(),
            cwd: "/tmp/worktree".to_string(),
            rollout_path: None,
            repo_root: None,
            git: None,
            resumed_from: None,
            forked_from: None,
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            approval_policy: "never".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            active_profile: None,
            config_snapshot: None,
            log_user_prompt: false,
            log_assistant_text: true,
            log_tool_output: true,
            log_diffs: true,
            hash_prompts: true,
            write_run_summary: true,
            capture_session: true,
            capture_turns: true,
            capture_usage: true,
            capture_tool_calls: true,
            capture_approvals: true,
            capture_errors: true,
        },
    );
    let thread_store = ExtensionData::new("thread-turn-items");
    let turn_store = ExtensionData::new("turn-turn-items");
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    install(&mut builder);
    let registry = builder.build();
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: false,
            environments: &[],
            mcp_resource_client: None,
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    let contributor = &registry.turn_item_contributors()[0];
    let mut assistant_item = TurnItem::AgentMessage(AgentMessageItem {
        id: "assistant-2".to_string(),
        content: vec![AgentMessageContent::Text {
            text: "visible assistant text".to_string(),
        }],
        phase: None,
        memory_citation: None,
    });
    contributor
        .contribute(&thread_store, &turn_store, &mut assistant_item)
        .await
        .expect("assistant contribution should succeed");

    let mut file_change_item = TurnItem::FileChange(FileChangeItem {
        id: "patch-2".to_string(),
        changes: std::collections::HashMap::from([
            (
                PathBuf::from("src/lib.rs"),
                FileChange::Update {
                    unified_diff: "@@\n-old\n+new\n".to_string(),
                    move_path: None,
                },
            ),
            (
                PathBuf::from("README.md"),
                FileChange::Add {
                    content: "new readme".to_string(),
                },
            ),
        ]),
        status: Some(PatchApplyStatus::Completed),
        auto_approved: Some(false),
        stdout: Some("captured patch stdout".to_string()),
        stderr: Some("captured patch stderr".to_string()),
    });
    contributor
        .contribute(&thread_store, &turn_store, &mut file_change_item)
        .await
        .expect("file-change contribution should succeed");

    registry.thread_lifecycle_contributors()[0]
        .on_thread_stop(ThreadStopInput {
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;

    let events = writer.events().await;
    let turn_item_events = events
        .iter()
        .filter(|event| event.event_type == TelemetryEventType::TurnItemRecorded)
        .collect::<Vec<_>>();
    assert_eq!(turn_item_events.len(), 2);
    assert_eq!(
        turn_item_events[0].payload["assistant_text"],
        "visible assistant text"
    );
    assert_eq!(
        turn_item_events[1].payload["tool_output"]["stdout"],
        "captured patch stdout"
    );
    assert_eq!(
        turn_item_events[1].payload["diffs"]["src/lib.rs"]["type"],
        "update"
    );
    assert_eq!(
        turn_item_events[1].payload["diffs"]["README.md"]["type"],
        "add"
    );

    let summary = &writer.summaries().await[0];
    assert_eq!(summary.tool_summary.file_write_count, 2);
}

#[tokio::test]
async fn approval_events_update_summary_and_emit_records() {
    let harness = Harness::start().await;

    crate::record_approval_requested(&harness.thread_store, "turn-1", "approval-1", "exec");
    crate::record_approval_resolved(
        &harness.thread_store,
        "turn-1",
        "approval-1",
        "exec",
        true,
        "approved",
    );
    crate::record_approval_requested(
        &harness.thread_store,
        "turn-1",
        "approval-2",
        "request_permissions",
    );
    crate::record_approval_resolved(
        &harness.thread_store,
        "turn-1",
        "approval-2",
        "request_permissions",
        false,
        "denied",
    );

    tokio::task::yield_now().await;
    harness.stop().await;

    let events = harness.writer.events().await;
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>(),
        vec![
            TelemetryEventType::SessionStarted,
            TelemetryEventType::ApprovalRecorded,
            TelemetryEventType::ApprovalRecorded,
            TelemetryEventType::ApprovalRecorded,
            TelemetryEventType::ApprovalRecorded,
            TelemetryEventType::SessionCompleted,
        ]
    );
    assert_eq!(events[1].payload["approval_kind"], "exec");
    assert_eq!(events[1].payload["phase"], "requested");
    assert_eq!(events[2].payload["phase"], "approved");
    assert_eq!(events[2].payload["decision"], "approved");
    assert_eq!(events[3].payload["approval_kind"], "request_permissions");
    assert_eq!(events[4].payload["phase"], "denied");
    assert_eq!(events[4].payload["decision"], "denied");

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.approval_summary.total_requests, 2);
    assert_eq!(summary.approval_summary.approved_count, 1);
    assert_eq!(summary.approval_summary.denied_count, 1);
}
