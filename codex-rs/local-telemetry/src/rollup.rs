use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::SessionSummary;
use crate::TELEMETRY_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DailyRollup {
    pub schema_version: u32,
    pub date: String,
    pub totals: RollupBucket,
    pub by_model: BTreeMap<String, RollupBucket>,
    pub by_effort: BTreeMap<String, RollupBucket>,
    pub by_repo: BTreeMap<String, RollupBucket>,
    pub by_mode: BTreeMap<String, RollupBucket>,
    #[serde(default)]
    pub by_task_type: BTreeMap<String, RollupBucket>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollupBucket {
    pub sessions: u64,
    pub turns: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub tool_calls: u64,
    #[serde(default)]
    pub shell_commands: u64,
    pub approvals: u64,
    pub failures: u64,
    pub duration_ms: u64,
}

impl DailyRollup {
    pub fn new(date: String) -> Self {
        Self {
            schema_version: TELEMETRY_SCHEMA_VERSION,
            date,
            totals: RollupBucket::default(),
            by_model: BTreeMap::new(),
            by_effort: BTreeMap::new(),
            by_repo: BTreeMap::new(),
            by_mode: BTreeMap::new(),
            by_task_type: BTreeMap::new(),
        }
    }

    pub fn add_summary(&mut self, summary: &SessionSummary) {
        self.totals.add_summary(summary);

        bucket_for_key(
            &mut self.by_model,
            summary.model.as_deref().unwrap_or("unknown"),
        )
        .add_summary(summary);
        bucket_for_key(
            &mut self.by_effort,
            summary.reasoning_effort.as_deref().unwrap_or("unknown"),
        )
        .add_summary(summary);
        bucket_for_key(
            &mut self.by_repo,
            summary
                .repo_root
                .as_deref()
                .or(summary.cwd.as_deref())
                .unwrap_or("unknown"),
        )
        .add_summary(summary);
        bucket_for_key(&mut self.by_mode, summary.invocation_mode.as_str()).add_summary(summary);
        for task_type in &summary.task_types {
            bucket_for_key(&mut self.by_task_type, task_type).add_summary(summary);
        }
    }
}

impl RollupBucket {
    fn add_summary(&mut self, summary: &SessionSummary) {
        self.sessions += 1;
        self.turns += summary.turn_counts.started;
        self.input_tokens += summary.usage_totals.input_tokens;
        self.cached_input_tokens += summary.usage_totals.cached_input_tokens;
        self.output_tokens += summary.usage_totals.output_tokens;
        self.reasoning_tokens += summary.usage_totals.reasoning_tokens;
        self.total_tokens += summary.usage_totals.total_tokens;
        self.tool_calls += summary.tool_summary.total_calls;
        self.shell_commands += summary.tool_summary.shell_command_count;
        self.approvals += summary.approval_summary.total_requests;
        self.failures += summary.turn_counts.aborted + summary.turn_counts.errored;
        self.duration_ms += summary.duration_ms.unwrap_or(0);
    }
}

fn bucket_for_key<'a>(
    buckets: &'a mut BTreeMap<String, RollupBucket>,
    key: &str,
) -> &'a mut RollupBucket {
    buckets.entry(key.to_string()).or_default()
}
