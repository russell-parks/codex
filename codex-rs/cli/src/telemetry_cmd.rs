use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use clap::ValueEnum;
use codex_config::types::OtelExporterKind;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_core::config::LoaderOverrides;
use codex_local_telemetry::LocalTelemetryStore;
use codex_local_telemetry::SessionSummary;
use codex_utils_cli::CliConfigOverrides;
use serde::Serialize;

#[derive(Debug, clap::Parser)]
pub struct TelemetryCli {
    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,

    #[command(subcommand)]
    pub subcommand: TelemetrySubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum TelemetrySubcommand {
    Status(StatusArgs),
    List(ListArgs),
    Show(ShowArgs),
    Report(ReportArgs),
    Export(ExportArgs),
    Prune(PruneArgs),
    Doctor(DoctorArgs),
}

#[derive(Debug, clap::Args, Default)]
pub struct StatusArgs {
    #[arg(long, value_enum, default_value_t = StatusFormat::Table)]
    pub format: StatusFormat,
}

#[derive(Debug, clap::Args, Default)]
pub struct ListArgs {
    #[arg(long, value_name = "DURATION")]
    pub since: Option<String>,

    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    pub session_id: String,

    #[arg(long, value_enum, default_value_t = ShowFormat::Pretty)]
    pub format: ShowFormat,
}

#[derive(Debug, clap::Args)]
pub struct ReportArgs {
    #[arg(long, value_name = "DURATION")]
    pub since: Option<String>,

    #[arg(long, value_enum, default_value_t = GroupBy::Day)]
    pub group_by: GroupBy,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, clap::Args)]
pub struct ExportArgs {
    #[arg(long, value_name = "DURATION")]
    pub since: Option<String>,

    #[arg(long, value_enum, default_value_t = ExportFormat::Json)]
    pub format: ExportFormat,

    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct PruneArgs {
    #[arg(long = "older-than", value_name = "DURATION")]
    pub older_than: String,
}

#[derive(Debug, clap::Args, Default)]
pub struct DoctorArgs {
    #[arg(long, value_enum, default_value_t = StatusFormat::Table)]
    pub format: StatusFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum ShowFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum ExportFormat {
    Jsonl,
    #[default]
    Json,
    Csv,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum GroupBy {
    Model,
    Effort,
    Repo,
    Mode,
    #[default]
    Day,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum StatusFormat {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Clone)]
struct LoadedTelemetry {
    config: Config,
    store: LocalTelemetryStore,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusView {
    enabled: bool,
    directory: String,
    retention_days: i64,
    privacy: PrivacyView,
    otel_configured: bool,
    disk_usage_bytes: u64,
    latest_event_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyView {
    log_user_prompt: bool,
    log_assistant_text: bool,
    log_tool_output: bool,
    log_diffs: bool,
    hash_prompts: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionRow {
    session_id: String,
    started_at: String,
    invocation_mode: String,
    model: Option<String>,
    repo_root: Option<String>,
    total_tokens: u64,
    tool_calls: u64,
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRow {
    key: String,
    sessions: u64,
    total_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    tool_calls: u64,
    duration_ms: u64,
}

impl TelemetryCli {
    pub async fn run(self, loader_overrides: LoaderOverrides) -> Result<()> {
        let TelemetryCli {
            mut config_overrides,
            subcommand,
        } = self;
        let loaded = load_telemetry(&mut config_overrides, loader_overrides).await?;

        match subcommand {
            TelemetrySubcommand::Status(args) => run_status(&loaded, args)?,
            TelemetrySubcommand::List(args) => run_list(&loaded, args)?,
            TelemetrySubcommand::Show(args) => run_show(&loaded, args)?,
            TelemetrySubcommand::Report(args) => run_report(&loaded, args)?,
            TelemetrySubcommand::Export(args) => run_export(&loaded, args)?,
            TelemetrySubcommand::Prune(args) => run_prune(&loaded, args)?,
            TelemetrySubcommand::Doctor(args) => run_doctor(&loaded, args)?,
        }

        Ok(())
    }
}

async fn load_telemetry(
    config_overrides: &mut CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<LoadedTelemetry> {
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let config = ConfigBuilder::default()
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .build()
        .await?;
    let telemetry_root = resolve_telemetry_root(&config);
    Ok(LoadedTelemetry {
        config,
        store: LocalTelemetryStore::new(telemetry_root),
    })
}

fn run_status(loaded: &LoadedTelemetry, args: StatusArgs) -> Result<()> {
    let view = StatusView {
        enabled: loaded.config.telemetry.local.enabled,
        directory: loaded.store.root().display().to_string(),
        retention_days: loaded.config.telemetry.local.retention_days,
        privacy: PrivacyView {
            log_user_prompt: loaded.config.telemetry.local.log_user_prompt,
            log_assistant_text: loaded.config.telemetry.local.log_assistant_text,
            log_tool_output: loaded.config.telemetry.local.log_tool_output,
            log_diffs: loaded.config.telemetry.local.log_diffs,
            hash_prompts: loaded.config.telemetry.local.hash_prompts,
        },
        otel_configured: otel_configured(&loaded.config),
        disk_usage_bytes: loaded.store.disk_usage_bytes()?,
        latest_event_timestamp: loaded.store.latest_event_timestamp()?,
    };

    match args.format {
        StatusFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&view)?);
        }
        StatusFormat::Table => {
            println!("enabled: {}", view.enabled);
            println!("directory: {}", view.directory);
            println!("retention_days: {}", view.retention_days);
            println!("otel_configured: {}", view.otel_configured);
            println!("disk_usage_bytes: {}", view.disk_usage_bytes);
            println!(
                "latest_event_timestamp: {}",
                view.latest_event_timestamp.as_deref().unwrap_or("-")
            );
            println!("privacy.log_user_prompt: {}", view.privacy.log_user_prompt);
            println!(
                "privacy.log_assistant_text: {}",
                view.privacy.log_assistant_text
            );
            println!("privacy.log_tool_output: {}", view.privacy.log_tool_output);
            println!("privacy.log_diffs: {}", view.privacy.log_diffs);
            println!("privacy.hash_prompts: {}", view.privacy.hash_prompts);
        }
    }

    Ok(())
}

fn run_list(loaded: &LoadedTelemetry, args: ListArgs) -> Result<()> {
    let summaries = filter_summaries(
        loaded.store.list_summaries()?,
        args.since.as_deref(),
        args.repo.as_deref(),
        args.model.as_deref(),
    )?;
    let rows = summaries.into_iter().map(summary_row).collect::<Vec<_>>();

    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
        OutputFormat::Table => render_session_rows(&rows),
    }

    Ok(())
}

fn run_show(loaded: &LoadedTelemetry, args: ShowArgs) -> Result<()> {
    let summary = loaded
        .store
        .read_summary(&args.session_id)?
        .with_context(|| format!("no telemetry summary found for session {}", args.session_id))?;

    match args.format {
        ShowFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
        ShowFormat::Pretty => {
            println!("session_id: {}", summary.session_id);
            println!("started_at: {}", summary.started_at);
            println!("ended_at: {}", summary.ended_at.as_deref().unwrap_or("-"));
            println!("invocation_mode: {}", summary.invocation_mode);
            println!("model: {}", summary.model.as_deref().unwrap_or("-"));
            println!(
                "reasoning_effort: {}",
                summary.reasoning_effort.as_deref().unwrap_or("-")
            );
            println!("cwd: {}", summary.cwd.as_deref().unwrap_or("-"));
            println!("repo_root: {}", summary.repo_root.as_deref().unwrap_or("-"));
            println!("total_tokens: {}", summary.usage_totals.total_tokens);
            println!(
                "cached_input_tokens: {}",
                summary.usage_totals.cached_input_tokens
            );
            println!("tool_calls: {}", summary.tool_summary.total_calls);
            println!("raw_event_path: {}", summary.raw_event_path);
            println!(
                "rollout_path: {}",
                summary.rollout_path.as_deref().unwrap_or("-")
            );
        }
    }

    Ok(())
}

fn run_report(loaded: &LoadedTelemetry, args: ReportArgs) -> Result<()> {
    let summaries = filter_summaries(
        loaded.store.list_summaries()?,
        args.since.as_deref(),
        None,
        None,
    )?;
    let rows = build_report_rows(&summaries, args.group_by);

    match args.format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
        OutputFormat::Table => render_report_rows(&rows),
    }

    Ok(())
}

fn run_export(loaded: &LoadedTelemetry, args: ExportArgs) -> Result<()> {
    let summaries = filter_summaries(
        loaded.store.list_summaries()?,
        args.since.as_deref(),
        None,
        None,
    )?;
    let payload = match args.format {
        ExportFormat::Jsonl => export_jsonl(&summaries)?,
        ExportFormat::Json => serde_json::to_string_pretty(&summaries)?,
        ExportFormat::Csv => export_csv(&summaries),
    };

    if let Some(output) = args.output {
        fs::write(&output, payload)
            .with_context(|| format!("failed to write {}", output.display()))?;
    } else {
        print!("{payload}");
        if !payload.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}

fn run_prune(loaded: &LoadedTelemetry, args: PruneArgs) -> Result<()> {
    let duration = parse_duration(args.older_than.as_str())?;
    let cutoff = chrono::Utc::now() - duration;
    let result = loaded.store.prune_older_than(cutoff)?;
    println!("removed_summaries: {}", result.removed_summaries);
    println!("removed_event_files: {}", result.removed_event_files);
    Ok(())
}

fn run_doctor(loaded: &LoadedTelemetry, args: DoctorArgs) -> Result<()> {
    let summaries = loaded.store.list_summaries()?;
    let view = serde_json::json!({
        "enabled": loaded.config.telemetry.local.enabled,
        "directory_exists": loaded.store.root().exists(),
        "summaries": summaries.len(),
        "latest_event_timestamp": loaded.store.latest_event_timestamp()?,
        "disk_usage_bytes": loaded.store.disk_usage_bytes()?,
    });

    match args.format {
        StatusFormat::Json => println!("{}", serde_json::to_string_pretty(&view)?),
        StatusFormat::Table => {
            println!("directory_exists: {}", loaded.store.root().exists());
            println!("summaries: {}", summaries.len());
            println!(
                "latest_event_timestamp: {}",
                view["latest_event_timestamp"].as_str().unwrap_or("-")
            );
            println!("disk_usage_bytes: {}", view["disk_usage_bytes"]);
        }
    }

    Ok(())
}

fn render_session_rows(rows: &[SessionRow]) {
    println!(
        "{:<36} {:<20} {:<12} {:<16} {:>12} {:>10}",
        "Session", "Started", "Mode", "Model", "Tokens", "Tools"
    );
    for row in rows {
        println!(
            "{:<36} {:<20} {:<12} {:<16} {:>12} {:>10}",
            row.session_id,
            truncate(&row.started_at, 20),
            truncate(&row.invocation_mode, 12),
            truncate(row.model.as_deref().unwrap_or("-"), 16),
            row.total_tokens,
            row.tool_calls,
        );
    }
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

fn filter_summaries(
    summaries: Vec<SessionSummary>,
    since: Option<&str>,
    repo: Option<&Path>,
    model: Option<&str>,
) -> Result<Vec<SessionSummary>> {
    let since = since.map(parse_duration).transpose()?;
    let cutoff = since.map(|duration| chrono::Utc::now() - duration);
    let repo = repo.map(|value| value.to_path_buf());

    let mut filtered = Vec::new();
    for summary in summaries {
        if let Some(cutoff) = cutoff {
            let started_at = chrono::DateTime::parse_from_rfc3339(&summary.started_at)
                .map_err(std::io::Error::other)?
                .with_timezone(&chrono::Utc);
            if started_at < cutoff {
                continue;
            }
        }
        if let Some(repo) = repo.as_ref() {
            let matches_repo = summary
                .repo_root
                .as_deref()
                .map(PathBuf::from)
                .or_else(|| summary.cwd.as_deref().map(PathBuf::from))
                .is_some_and(|value| value.starts_with(repo));
            if !matches_repo {
                continue;
            }
        }
        if let Some(model) = model
            && summary.model.as_deref() != Some(model)
        {
            continue;
        }
        filtered.push(summary);
    }

    Ok(filtered)
}

fn summary_row(summary: SessionSummary) -> SessionRow {
    SessionRow {
        session_id: summary.session_id,
        started_at: summary.started_at,
        invocation_mode: summary.invocation_mode,
        model: summary.model,
        repo_root: summary.repo_root,
        total_tokens: summary.usage_totals.total_tokens,
        tool_calls: summary.tool_summary.total_calls,
        duration_ms: summary.duration_ms,
    }
}

fn build_report_rows(summaries: &[SessionSummary], group_by: GroupBy) -> Vec<ReportRow> {
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

fn export_jsonl(summaries: &[SessionSummary]) -> Result<String> {
    let mut output = String::new();
    for summary in summaries {
        let payload = fs::read_to_string(&summary.raw_event_path)
            .with_context(|| format!("failed to read {}", summary.raw_event_path))?;
        output.push_str(&payload);
        if !payload.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output)
}

fn export_csv(summaries: &[SessionSummary]) -> String {
    let mut output =
        String::from("session_id,started_at,invocation_mode,model,total_tokens,tool_calls\n");
    for summary in summaries {
        let _ = writeln!(
            output,
            "{},{},{},{},{},{}",
            csv_field(&summary.session_id),
            csv_field(&summary.started_at),
            csv_field(&summary.invocation_mode),
            csv_field(summary.model.as_deref().unwrap_or("")),
            summary.usage_totals.total_tokens,
            summary.tool_summary.total_calls,
        );
    }
    output
}

fn csv_field(value: &str) -> String {
    let escaped = value.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn otel_configured(config: &Config) -> bool {
    !matches!(config.otel.exporter, OtelExporterKind::None)
        || !matches!(config.otel.trace_exporter, OtelExporterKind::None)
        || !matches!(config.otel.metrics_exporter, OtelExporterKind::None)
}

fn parse_duration(value: &str) -> Result<Duration> {
    let split_at = value
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(value.len());
    let amount: u64 = value[..split_at]
        .parse()
        .with_context(|| format!("invalid duration value `{value}`"))?;
    let unit = &value[split_at..];
    let seconds = match unit {
        "s" => amount,
        "m" => amount * 60,
        "h" => amount * 60 * 60,
        "d" => amount * 60 * 60 * 24,
        "w" => amount * 60 * 60 * 24 * 7,
        _ => anyhow::bail!("unsupported duration unit in `{value}`"),
    };
    Ok(Duration::from_secs(seconds))
}

fn resolve_telemetry_root(config: &Config) -> PathBuf {
    let configured = &config.telemetry.local.directory;
    if let Some(stripped) = configured.strip_prefix("~/")
        && let Some(home_dir) = config.codex_home.parent()
    {
        return home_dir.join(stripped).to_path_buf();
    }

    let configured_path = PathBuf::from(configured);
    if configured_path.is_absolute() {
        configured_path
    } else {
        config.codex_home.join(configured_path).to_path_buf()
    }
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }
    value
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>()
        + "..."
}

#[cfg(test)]
#[path = "telemetry_cmd_tests.rs"]
mod tests;
