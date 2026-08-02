use codex_local_telemetry::TelemetryEventType;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::McpToolCallItem;
use codex_protocol::items::TurnItem;
use serde_json::json;

use super::recording::append_event;
use crate::state::LocalTelemetryRunState;

pub(super) async fn record_turn_item(
    run_state: &LocalTelemetryRunState,
    turn_id: &str,
    item: &TurnItem,
) {
    update_summary_for_turn_item(run_state, item).await;

    let payload = match item {
        TurnItem::AgentMessage(agent_message) if run_state.log_assistant_text => Some(json!({
            "item_id": agent_message.id,
            "turn_item_type": "agent_message",
            "assistant_text": assistant_text(agent_message),
            "phase": agent_message.phase,
        })),
        TurnItem::FileChange(file_change) if run_state.log_tool_output || run_state.log_diffs => {
            Some(json!({
                "item_id": file_change.id,
                "turn_item_type": "file_change",
                "status": file_change.status,
                "tool_output": run_state.log_tool_output.then(|| json!({
                    "stdout": file_change.stdout,
                    "stderr": file_change.stderr,
                    "auto_approved": file_change.auto_approved,
                })),
                "diffs": run_state
                    .log_diffs
                    .then(|| serde_json::to_value(&file_change.changes).unwrap_or_default()),
            }))
        }
        TurnItem::McpToolCall(tool_call) if run_state.log_tool_output => Some(json!({
            "item_id": tool_call.id,
            "turn_item_type": "mcp_tool_call",
            "status": tool_call.status,
            "tool_output": mcp_tool_output(tool_call),
        })),
        _ => None,
    };

    let Some(payload) = payload else {
        return;
    };

    append_event(
        run_state,
        TelemetryEventType::TurnItemRecorded,
        Some(turn_id),
        payload,
    )
    .await;
}

async fn update_summary_for_turn_item(run_state: &LocalTelemetryRunState, item: &TurnItem) {
    let file_write_count = match item {
        TurnItem::FileChange(FileChangeItem {
            changes, status, ..
        }) if status.is_some() => u64::try_from(changes.len()).unwrap_or(u64::MAX),
        _ => 0,
    };

    if file_write_count == 0 {
        return;
    }

    let mut summary = run_state.summary.lock().await;
    summary.tool_summary.file_write_count = summary
        .tool_summary
        .file_write_count
        .saturating_add(file_write_count);
}

fn assistant_text(agent_message: &codex_protocol::items::AgentMessageItem) -> String {
    agent_message
        .content
        .iter()
        .map(|content| match content {
            AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect()
}

fn mcp_tool_output(tool_call: &McpToolCallItem) -> serde_json::Value {
    json!({
        "server": tool_call.server,
        "tool": tool_call.tool,
        "result": tool_call.result,
        "error": tool_call.error,
    })
}
