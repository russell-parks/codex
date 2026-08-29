use serde::Serialize;

use super::DailyRollup;
use super::GroupBy;
use super::SessionSummary;
use super::truncate;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReportRow {
    pub(crate) key: String,
    pub(crate) sessions: u64,
    pub(crate) total_tokens: u64,
    pub(crate) cached_input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) reasoning_tokens: u64,
    pub(crate) tool_calls: u64,
    pub(crate) duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReportView {
    pub(crate) group_by: String,
    pub(crate) rows: Vec<ReportRow>,
    pub(crate) insights: ReportInsights,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReportInsights {
    pub(crate) high_token_without_changes: Vec<SessionInsightRow>,
    pub(crate) high_reasoning_sessions: Vec<SessionInsightRow>,
    pub(crate) many_tool_call_sessions: Vec<SessionInsightRow>,
    pub(crate) high_token_failures: Vec<SessionInsightRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionInsightRow {
    pub(crate) session_id: String,
    pub(crate) model: Option<String>,
    pub(crate) repo_root: Option<String>,
    pub(crate) total_tokens: u64,
    pub(crate) reasoning_tokens: u64,
    pub(crate) cached_input_tokens: u64,
    pub(crate) tool_calls: u64,
    pub(crate) error_count: u64,
    pub(crate) aborted_turns: u64,
    pub(crate) changed_files: u64,
}

fn render_report_rows(rows: &[ReportRow]) {
    println!(
        "{:<24} {:>8} {:>12} {:>12} {:>12} {:>12}",
        "Group", "Sessions", "Tokens", "Cached", "Reasoning", "Tools"
    );
    for row in rows {
        println!(
            "{:<24} {:>8} {:>12} {:>12} {:>12} {:>12}",
            truncate(&row.key, 24),
            row.sessions,
            row.total_tokens,
            row.cached_input_tokens,
            row.reasoning_tokens,
            row.tool_calls,
        );
    }
}

pub(super) fn render_report_view(view: &ReportView) {
    render_report_rows(&view.rows);
    render_insight_block(
        "High-token sessions without file changes",
        &view.insights.high_token_without_changes,
    );
    render_insight_block(
        "High reasoning-token sessions",
        &view.insights.high_reasoning_sessions,
    );
    render_insight_block(
        "Many tool-call sessions",
        &view.insights.many_tool_call_sessions,
    );
    render_insight_block(
        "High-token failed or aborted sessions",
        &view.insights.high_token_failures,
    );
}

fn render_insight_block(title: &str, rows: &[SessionInsightRow]) {
    if rows.is_empty() {
        return;
    }

    println!();
    println!("{title}:");
    for row in rows {
        println!(
            "  {} tokens={} reasoning={} cached={} tools={} errors={} aborted={} changed_files={}",
            truncate(&row.session_id, 24),
            row.total_tokens,
            row.reasoning_tokens,
            row.cached_input_tokens,
            row.tool_calls,
            row.error_count,
            row.aborted_turns,
            row.changed_files,
        );
    }
}

pub(crate) fn build_report_rows(summaries: &[SessionSummary], group_by: GroupBy) -> Vec<ReportRow> {
    let mut by_key = std::collections::BTreeMap::<String, ReportRow>::new();
    for summary in summaries {
        let key = report_key(summary, group_by);
        let row = by_key.entry(key.clone()).or_insert_with(|| ReportRow {
            key,
            sessions: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            tool_calls: 0,
            duration_ms: 0,
        });
        row.sessions += 1;
        row.total_tokens += summary.usage_totals.total_tokens;
        row.cached_input_tokens += summary.usage_totals.cached_input_tokens;
        row.output_tokens += summary.usage_totals.output_tokens;
        row.reasoning_tokens += summary.usage_totals.reasoning_tokens;
        row.tool_calls += summary.tool_summary.total_calls;
        row.duration_ms += summary.duration_ms.unwrap_or(0);
    }

    by_key.into_values().collect()
}

pub(crate) fn build_report_rows_from_rollups(rollups: &[DailyRollup]) -> Vec<ReportRow> {
    rollups
        .iter()
        .map(|rollup| ReportRow {
            key: rollup.date.clone(),
            sessions: rollup.totals.sessions,
            total_tokens: rollup.totals.total_tokens,
            cached_input_tokens: rollup.totals.cached_input_tokens,
            output_tokens: rollup.totals.output_tokens,
            reasoning_tokens: rollup.totals.reasoning_tokens,
            tool_calls: rollup.totals.tool_calls,
            duration_ms: rollup.totals.duration_ms,
        })
        .collect()
}

pub(crate) fn build_report_insights(summaries: &[SessionSummary]) -> ReportInsights {
    ReportInsights {
        high_token_without_changes: top_sessions(
            summaries.iter().filter(|summary| {
                summary.usage_totals.total_tokens > 0
                    && summary.changed_files_summary.paths.is_empty()
            }),
            |summary| summary.usage_totals.total_tokens,
        ),
        high_reasoning_sessions: top_sessions(
            summaries
                .iter()
                .filter(|summary| summary.usage_totals.reasoning_tokens > 0),
            |summary| summary.usage_totals.reasoning_tokens,
        ),
        many_tool_call_sessions: top_sessions(
            summaries
                .iter()
                .filter(|summary| summary.tool_summary.total_calls > 0),
            |summary| summary.tool_summary.total_calls,
        ),
        high_token_failures: top_sessions(
            summaries.iter().filter(|summary| {
                summary.usage_totals.total_tokens > 0
                    && (summary.error_summary.error_count > 0 || summary.turn_counts.aborted > 0)
            }),
            |summary| summary.usage_totals.total_tokens,
        ),
    }
}

fn top_sessions<'a, I, F>(summaries: I, score: F) -> Vec<SessionInsightRow>
where
    I: Iterator<Item = &'a SessionSummary>,
    F: Fn(&SessionSummary) -> u64,
{
    let mut rows = summaries
        .map(|summary| (score(summary), session_insight_row(summary)))
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.session_id.cmp(&right.1.session_id))
    });
    rows.truncate(5);
    rows.into_iter().map(|(_, row)| row).collect()
}

fn session_insight_row(summary: &SessionSummary) -> SessionInsightRow {
    SessionInsightRow {
        session_id: summary.session_id.clone(),
        model: summary.model.clone(),
        repo_root: summary.repo_root.clone(),
        total_tokens: summary.usage_totals.total_tokens,
        reasoning_tokens: summary.usage_totals.reasoning_tokens,
        cached_input_tokens: summary.usage_totals.cached_input_tokens,
        tool_calls: summary.tool_summary.total_calls,
        error_count: summary.error_summary.error_count,
        aborted_turns: summary.turn_counts.aborted,
        changed_files: u64::try_from(summary.changed_files_summary.paths.len()).unwrap_or(u64::MAX),
    }
}

fn report_key(summary: &SessionSummary, group_by: GroupBy) -> String {
    match group_by {
        GroupBy::Model => summary
            .model
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        GroupBy::Effort => summary
            .reasoning_effort
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        GroupBy::Repo => summary
            .repo_root
            .clone()
            .or_else(|| summary.cwd.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        GroupBy::Mode => summary.invocation_mode.clone(),
        GroupBy::Day => summary
            .started_at
            .split('T')
            .next()
            .unwrap_or("unknown")
            .to_string(),
    }
}
