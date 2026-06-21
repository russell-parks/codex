use super::*;

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

    let run_state = run_state;
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

    let run_state = run_state;
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

pub fn record_turn_profile(session_store: &ExtensionData, turn_id: &str, profile: &TurnProfile) {
    let Some(run_state) = session_store.get::<LocalTelemetryRunState>() else {
        return;
    };

    let run_state = run_state;
    let turn_id = turn_id.to_string();
    let profile = profile.clone();
    tokio::spawn(async move {
        {
            let mut summary = run_state.summary.lock().await;
            summary.runtime_summary.api_request_count = summary
                .runtime_summary
                .api_request_count
                .saturating_add(u64::from(profile.sampling_request_count));
            summary.runtime_summary.retry_count = summary
                .runtime_summary
                .retry_count
                .saturating_add(u64::from(profile.sampling_retry_count));
        }

        append_event(
            run_state.as_ref(),
            TelemetryEventType::TurnProfileRecorded,
            Some(turn_id.as_str()),
            json!({
                "turn_id": turn_id,
                "sampling_request_count": profile.sampling_request_count,
                "sampling_retry_count": profile.sampling_retry_count,
                "before_first_sampling_ms": profile.before_first_sampling_ms,
                "sampling_ms": profile.sampling_ms,
                "between_sampling_overhead_ms": profile.between_sampling_overhead_ms,
                "tool_blocking_ms": profile.tool_blocking_ms,
                "after_last_sampling_ms": profile.after_last_sampling_ms,
            }),
        )
        .await;
    });
}

pub fn record_rate_limits(
    session_store: &ExtensionData,
    turn_id: &str,
    rate_limits: &RateLimitSnapshot,
) {
    let Some(run_state) = session_store.get::<LocalTelemetryRunState>() else {
        return;
    };

    let run_state = run_state;
    let turn_id = turn_id.to_string();
    let summary_value = rate_limit_summary(rate_limits);
    tokio::spawn(async move {
        {
            let mut summary = run_state.summary.lock().await;
            summary.runtime_summary.latest_rate_limits = Some(summary_value.clone());
        }

        append_event(
            run_state.as_ref(),
            TelemetryEventType::RateLimitsRecorded,
            Some(turn_id.as_str()),
            json!({
                "turn_id": turn_id,
                "rate_limits": summary_value,
            }),
        )
        .await;
    });
}

pub fn record_task_type(session_store: &ExtensionData, task_type: &str) {
    let Some(run_state) = session_store.get::<LocalTelemetryRunState>() else {
        return;
    };

    let run_state = run_state;
    let task_type = task_type.to_string();
    tokio::spawn(async move {
        let mut summary = run_state.summary.lock().await;
        if !summary.task_types.iter().any(|value| value == &task_type) {
            summary.task_types.push(task_type);
            summary.task_types.sort();
        }
    });
}

pub(super) async fn append_event(
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

pub(super) fn tool_call_outcome_payload(outcome: ToolCallOutcome) -> serde_json::Value {
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

pub(super) fn take_prompt_metadata(
    session_store: &ExtensionData,
    turn_id: &str,
) -> Option<PromptMetadataSummary> {
    session_store
        .get::<PromptCaptureState>()
        .and_then(|state| state.remove(turn_id))
}

pub(super) fn increment_tool_classification(
    summary: &mut codex_local_telemetry::ToolSummary,
    tool_name: &str,
) {
    match tool_name {
        "exec_command" => {
            summary.shell_command_count += 1;
        }
        "apply_patch" => {
            summary.file_edit_count += 1;
        }
        "view_image" | "read_mcp_resource" => {
            summary.file_read_count += 1;
        }
        _ => {}
    }
}

fn rate_limit_summary(rate_limits: &RateLimitSnapshot) -> RateLimitSummary {
    RateLimitSummary {
        limit_id: rate_limits.limit_id.clone(),
        limit_name: rate_limits.limit_name.clone(),
        primary: rate_limits.primary.as_ref().map(rate_limit_window_summary),
        secondary: rate_limits
            .secondary
            .as_ref()
            .map(rate_limit_window_summary),
        credits: rate_limits.credits.as_ref().map(|credits| {
            codex_local_telemetry::CreditsSummary {
                has_credits: credits.has_credits,
                unlimited: credits.unlimited,
                balance: credits.balance.clone(),
            }
        }),
        plan_type: rate_limits.plan_type.map(plan_type_summary),
        rate_limit_reached_type: rate_limits
            .rate_limit_reached_type
            .map(rate_limit_reached_type_summary),
    }
}

fn rate_limit_window_summary(
    window: &codex_protocol::protocol::RateLimitWindow,
) -> RateLimitWindowSummary {
    RateLimitWindowSummary {
        used_percent: window.used_percent,
        window_minutes: window.window_minutes,
        resets_at: window.resets_at,
    }
}

fn plan_type_summary(plan_type: codex_protocol::account::PlanType) -> String {
    match plan_type {
        codex_protocol::account::PlanType::Free => String::from("free"),
        codex_protocol::account::PlanType::Go => String::from("go"),
        codex_protocol::account::PlanType::Plus => String::from("plus"),
        codex_protocol::account::PlanType::Pro => String::from("pro"),
        codex_protocol::account::PlanType::ProLite => String::from("pro_lite"),
        codex_protocol::account::PlanType::Team => String::from("team"),
        codex_protocol::account::PlanType::SelfServeBusinessUsageBased => {
            String::from("self_serve_business_usage_based")
        }
        codex_protocol::account::PlanType::Business => String::from("business"),
        codex_protocol::account::PlanType::EnterpriseCbpUsageBased => {
            String::from("enterprise_cbp_usage_based")
        }
        codex_protocol::account::PlanType::Enterprise => String::from("enterprise"),
        codex_protocol::account::PlanType::Edu => String::from("edu"),
        codex_protocol::account::PlanType::Unknown => String::from("unknown"),
    }
}

fn rate_limit_reached_type_summary(
    reached_type: codex_protocol::protocol::RateLimitReachedType,
) -> String {
    match reached_type {
        codex_protocol::protocol::RateLimitReachedType::RateLimitReached => {
            String::from("rate_limit_reached")
        }
        codex_protocol::protocol::RateLimitReachedType::WorkspaceOwnerCreditsDepleted => {
            String::from("workspace_owner_credits_depleted")
        }
        codex_protocol::protocol::RateLimitReachedType::WorkspaceMemberCreditsDepleted => {
            String::from("workspace_member_credits_depleted")
        }
        codex_protocol::protocol::RateLimitReachedType::WorkspaceOwnerUsageLimitReached => {
            String::from("workspace_owner_usage_limit_reached")
        }
        codex_protocol::protocol::RateLimitReachedType::WorkspaceMemberUsageLimitReached => {
            String::from("workspace_member_usage_limit_reached")
        }
    }
}
