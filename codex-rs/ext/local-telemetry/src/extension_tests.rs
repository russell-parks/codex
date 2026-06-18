use std::io;
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
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry::SessionSummary;
use codex_local_telemetry::TELEMETRY_SCHEMA_VERSION;
use codex_local_telemetry::TelemetryEvent;
use codex_local_telemetry::TelemetryEventType;
use codex_local_telemetry::UsageTotals;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TokenUsageInfo;
use codex_protocol::protocol::TurnAbortReason;
use pretty_assertions::assert_eq;
use tokio::sync::Mutex;

use crate::SessionTelemetryBootstrap;
use crate::initialize_session_data;
use crate::install;
use crate::record_user_prompt;
use crate::state::LocalTelemetryRunState;
use crate::state::PromptCaptureState;
use crate::update_session_stop_metadata;

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
                model: "gpt-5".to_string(),
                reasoning_effort: Some("medium".to_string()),
                approval_policy: "on-failure".to_string(),
                sandbox_mode: "workspace-write".to_string(),
                active_profile: Some("safe".to_string()),
                log_user_prompt: false,
                hash_prompts: true,
                write_run_summary: true,
                capture_session: true,
                capture_turns: true,
                capture_usage: true,
                capture_tool_calls: true,
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
            output_tokens: 4,
            reasoning_output_tokens: 1,
            total_tokens: 15,
        },
        last_token_usage: TokenUsage {
            input_tokens: 3,
            cached_input_tokens: 1,
            output_tokens: 2,
            reasoning_output_tokens: 1,
            total_tokens: 6,
        },
        model_context_window: Some(128_000),
    }
}

#[tokio::test]
async fn thread_lifecycle_writes_session_started_and_completed() {
    let harness = Harness::start().await;
    update_session_stop_metadata(
        &harness.session_store,
        Some("/tmp/final.rollout".to_string()),
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
    assert_eq!(events[1].payload["rollout_path"], "/tmp/final.rollout");

    let summaries = harness.writer.summaries().await;
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    let expected = SessionSummary {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        session_id: "thread-1".to_string(),
        started_at: summary.started_at.clone(),
        ended_at: summary.ended_at.clone(),
        duration_ms: summary.duration_ms,
        invocation_mode: "cli".to_string(),
        session_source: "cli".to_string(),
        model: Some("gpt-5".to_string()),
        reasoning_effort: Some("medium".to_string()),
        approval_policy: Some("on-failure".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        cwd: Some("/tmp/worktree".to_string()),
        repo_root: None,
        git: None,
        prompt_metadata: Default::default(),
        raw_event_path: "/tmp/raw-events.jsonl".to_string(),
        rollout_path: Some("/tmp/final.rollout".to_string()),
        usage_totals: Default::default(),
        turn_counts: Default::default(),
        tool_summary: Default::default(),
        approval_summary: Default::default(),
        error_summary: Default::default(),
        changed_files_summary: Default::default(),
        resumed_from: None,
        forked_from: None,
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
            TelemetryEventType::TurnCompleted,
            TelemetryEventType::SessionCompleted,
        ]
    );
    assert_eq!(events[2].turn_id.as_deref(), Some("turn-1"));
    assert_eq!(events[4].payload["outcome"]["kind"], "completed");
    assert_eq!(events[4].payload["outcome"]["success"], true);
    assert_eq!(
        events[5].payload["prompt_metadata"]["prompt_byte_length"],
        11
    );
    assert_eq!(
        events[5].payload["prompt_metadata"]["prompt_text"],
        serde_json::Value::Null
    );

    let summary = &harness.writer.summaries().await[0];
    assert_eq!(summary.turn_counts.started, 1);
    assert_eq!(summary.turn_counts.completed, 1);
    assert_eq!(summary.tool_summary.total_calls, 1);
    assert_eq!(summary.tool_summary.success_count, 1);
    assert_eq!(summary.tool_summary.failure_count, 0);
    assert_eq!(summary.usage_totals.input_tokens, 10);
    assert_eq!(summary.usage_totals.cached_input_tokens, 2);
    assert_eq!(summary.usage_totals.output_tokens, 4);
    assert_eq!(summary.usage_totals.reasoning_tokens, 1);
    assert_eq!(summary.usage_totals.total_tokens, 15);
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
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            approval_policy: "never".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            active_profile: None,
            log_user_prompt: true,
            hash_prompts: false,
            write_run_summary: false,
            capture_session: true,
            capture_turns: true,
            capture_usage: true,
            capture_tool_calls: true,
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
            model: "gpt-5".to_string(),
            reasoning_effort: Some("medium".to_string()),
            approval_policy: "on-failure".to_string(),
            sandbox_mode: "workspace-write".to_string(),
            active_profile: None,
            log_user_prompt: false,
            hash_prompts: true,
            write_run_summary: true,
            capture_session: true,
            capture_turns: true,
            capture_usage: false,
            capture_tool_calls: false,
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
