use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::agents_md::LoadedAgentsMd;
use chrono::Utc;
use codex_config::CONFIG_TOML_FILE;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStackOrdering;
use codex_config::format_config_layer_source;
use codex_extension_api::ExtensionData;
use codex_git_utils::canonicalize_git_remote_url;
use codex_git_utils::current_branch_name;
use codex_git_utils::get_git_remote_urls_assume_git_repo;
use codex_git_utils::get_git_repo_root;
use codex_git_utils::get_has_changes;
use codex_git_utils::get_head_commit_hash;
use codex_local_telemetry::ChangedFilesSummary;
use codex_local_telemetry::ConfigSnapshotSummary;
use codex_local_telemetry::ConfigSourceSummary;
use codex_local_telemetry::GitSummary;
use codex_local_telemetry::JsonlTelemetryWriter;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry_extension::SessionTelemetryBootstrap;
use codex_protocol::protocol::InitialHistory;
use tokio::process::Command;
use tokio::time::timeout;

use crate::ThreadConfigSnapshot;
use crate::config::Config;
use crate::session::session::Session;

pub(crate) async fn initialize_session_extension_data(
    config: &Config,
    thread_config: &ThreadConfigSnapshot,
    initial_history: &InitialHistory,
    developer_instructions_loaded: bool,
    loaded_agents_md: Option<&LoadedAgentsMd>,
    thread_id: &str,
    rollout_path: Option<&Path>,
    session_store: &ExtensionData,
) {
    if !config.telemetry.local.enabled {
        return;
    }

    let telemetry_root = resolve_telemetry_root(config);
    let writer = JsonlTelemetryWriter::new(
        telemetry_root,
        Utc::now().date_naive(),
        thread_id.to_string(),
    );
    let raw_event_path = writer.raw_event_path().display().to_string();
    let writer: Arc<dyn LocalTelemetryWriter> = Arc::new(writer);
    let (repo_root, git) = if config.telemetry.local.capture_git {
        collect_git_summary(thread_config.cwd().as_path()).await
    } else {
        (None, None)
    };
    let config_snapshot =
        config.telemetry.local.capture_config_snapshot.then(|| {
            build_config_snapshot(config, developer_instructions_loaded, loaded_agents_md)
        });
    let bootstrap = SessionTelemetryBootstrap {
        invocation_mode: thread_config.session_source.to_string(),
        cwd: thread_config.cwd().display().to_string(),
        rollout_path: rollout_path.map(path_to_string),
        repo_root,
        git,
        resumed_from: resumed_from(initial_history),
        forked_from: thread_config
            .forked_from_thread_id
            .map(|value| value.to_string()),
        model: thread_config.collaboration_mode.model().to_string(),
        reasoning_effort: thread_config
            .collaboration_mode
            .reasoning_effort()
            .map(|value| value.to_string()),
        approval_policy: thread_config.approval_policy.to_string(),
        sandbox_mode: format!("{:?}", thread_config.sandbox_policy()),
        active_profile: thread_config
            .active_permission_profile
            .as_ref()
            .map(|value| value.id.clone()),
        config_snapshot,
        log_user_prompt: config.telemetry.local.log_user_prompt,
        hash_prompts: config.telemetry.local.hash_prompts,
        write_run_summary: config.telemetry.local.write_run_summary,
        capture_session: config.telemetry.local.capture_session,
        capture_turns: config.telemetry.local.capture_turns,
        capture_usage: config.telemetry.local.capture_usage,
        capture_tool_calls: config.telemetry.local.capture_tool_calls,
        capture_approvals: config.telemetry.local.capture_approvals,
        capture_errors: config.telemetry.local.capture_errors,
    };
    codex_local_telemetry_extension::initialize_session_data(
        session_store,
        writer,
        raw_event_path,
        bootstrap,
    );
}

fn build_config_snapshot(
    config: &Config,
    developer_instructions_loaded: bool,
    loaded_agents_md: Option<&LoadedAgentsMd>,
) -> ConfigSnapshotSummary {
    let config_sources = config
        .config_layer_stack
        .get_layers(
            ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ false,
        )
        .into_iter()
        .map(|layer| ConfigSourceSummary {
            kind: config_source_kind(&layer.name).to_string(),
            source: format_config_layer_source(&layer.name, CONFIG_TOML_FILE),
            profile: match &layer.name {
                ConfigLayerSource::User { profile, .. } => profile.clone(),
                _ => None,
            },
        })
        .collect();
    let user_instruction_source = loaded_agents_md
        .and_then(LoadedAgentsMd::user_instructions)
        .map(|instructions| instructions.source.display().to_string());
    let project_instruction_sources = loaded_agents_md
        .into_iter()
        .flat_map(LoadedAgentsMd::sources)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    ConfigSnapshotSummary {
        config_sources,
        developer_instructions_loaded,
        user_instructions_loaded: user_instruction_source.is_some(),
        user_instruction_source,
        project_instructions_loaded: !project_instruction_sources.is_empty(),
        project_instruction_sources,
    }
}

fn config_source_kind(source: &ConfigLayerSource) -> &'static str {
    match source {
        ConfigLayerSource::Mdm { .. } => "mdm",
        ConfigLayerSource::System { .. } => "system",
        ConfigLayerSource::EnterpriseManaged { .. } => "enterprise_managed",
        ConfigLayerSource::User { .. } => "user",
        ConfigLayerSource::Project { .. } => "project",
        ConfigLayerSource::SessionFlags => "session_flags",
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { .. } => "legacy_managed_file",
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => "legacy_managed_mdm",
    }
}

pub(crate) async fn update_session_stop_metadata(session: &Session) {
    let cwd = session.cwd().await;
    let rollout_path = match session.current_rollout_path().await {
        Ok(path) => path.map(path_to_string),
        Err(err) => {
            tracing::warn!("failed to read local telemetry rollout path at shutdown: {err}");
            None
        }
    };
    let git = collect_git_stop_summary(cwd.as_path()).await;
    let changed_files_summary = collect_changed_files_summary(cwd.as_path()).await;
    codex_local_telemetry_extension::update_session_stop_metadata_with_details(
        &session.services.session_extension_data,
        rollout_path,
        git,
        changed_files_summary,
    );
}

pub(crate) fn record_user_prompt(session_store: &ExtensionData, turn_id: &str, prompt_text: &str) {
    codex_local_telemetry_extension::record_user_prompt(session_store, turn_id, prompt_text);
}

pub(crate) fn record_approval_requested(
    thread_store: &ExtensionData,
    turn_id: &str,
    approval_id: &str,
    approval_kind: &str,
) {
    codex_local_telemetry_extension::record_approval_requested(
        thread_store,
        turn_id,
        approval_id,
        approval_kind,
    );
}

pub(crate) fn record_approval_resolved(
    thread_store: &ExtensionData,
    turn_id: &str,
    approval_id: &str,
    approval_kind: &str,
    approved: bool,
    decision: &str,
) {
    codex_local_telemetry_extension::record_approval_resolved(
        thread_store,
        turn_id,
        approval_id,
        approval_kind,
        approved,
        decision,
    );
}

fn resolve_telemetry_root(config: &Config) -> PathBuf {
    let configured = &config.telemetry.local.directory;
    if let Some(stripped) = configured.strip_prefix("~/")
        && let Some(home_dir) = dirs::home_dir()
    {
        return home_dir.join(stripped);
    }

    let configured_path = PathBuf::from(configured);
    if configured_path.is_absolute() {
        configured_path
    } else {
        config.codex_home.join(configured_path).to_path_buf()
    }
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

async fn collect_git_summary(cwd: &Path) -> (Option<String>, Option<GitSummary>) {
    let Some(repo_root) = get_git_repo_root(cwd) else {
        return (None, None);
    };

    let repo_root_string = path_to_string(&repo_root);
    let (branch, commit_sha_before, dirty_before, remote_urls) = tokio::join!(
        current_branch_name(repo_root.as_path()),
        get_head_commit_hash(repo_root.as_path()),
        get_has_changes(repo_root.as_path()),
        get_git_remote_urls_assume_git_repo(repo_root.as_path()),
    );
    let remote = remote_urls.as_ref().and_then(select_remote_identity);
    let git = GitSummary {
        remote,
        branch,
        commit_sha_before: commit_sha_before.map(|value| value.0),
        commit_sha_after: None,
        dirty_before,
        dirty_after: None,
    };

    (Some(repo_root_string), Some(git))
}

async fn collect_git_stop_summary(cwd: &Path) -> Option<GitSummary> {
    let repo_root = get_git_repo_root(cwd)?;
    let (commit_sha_after, dirty_after) = tokio::join!(
        get_head_commit_hash(repo_root.as_path()),
        get_has_changes(repo_root.as_path()),
    );

    Some(GitSummary {
        remote: None,
        branch: None,
        commit_sha_before: None,
        commit_sha_after: commit_sha_after.map(|value| value.0),
        dirty_before: None,
        dirty_after,
    })
}

fn select_remote_identity(remotes: &std::collections::BTreeMap<String, String>) -> Option<String> {
    remotes
        .get("origin")
        .or_else(|| remotes.values().next())
        .and_then(|value| canonicalize_git_remote_url(value))
}

fn resumed_from(initial_history: &InitialHistory) -> Option<String> {
    match initial_history {
        InitialHistory::Resumed(resumed) => Some(resumed.conversation_id.to_string()),
        InitialHistory::New | InitialHistory::Cleared | InitialHistory::Forked(_) => None,
    }
}

const LOCAL_TELEMETRY_GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

async fn collect_changed_files_summary(cwd: &Path) -> Option<ChangedFilesSummary> {
    let repo_root = get_git_repo_root(cwd)?;
    let status_output = run_git_capture(
        repo_root.as_path(),
        &["status", "--short", "--untracked-files=all"],
    )
    .await?;
    let mut paths = parse_status_paths(&status_output);
    paths.sort();
    paths.dedup();

    let counts_by_extension = count_paths_by_extension(&paths);
    let (insertions, deletions) = collect_numstat_summary(repo_root.as_path()).await;

    Some(ChangedFilesSummary {
        paths,
        counts_by_extension,
        insertions,
        deletions,
    })
}

async fn collect_numstat_summary(repo_root: &Path) -> (Option<u64>, Option<u64>) {
    let Some(output) = run_git_capture(repo_root, &["diff", "--numstat", "HEAD", "--"]).await
    else {
        return (None, None);
    };

    let mut insertions = 0_u64;
    let mut deletions = 0_u64;
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        let Some(added) = parts.next() else {
            continue;
        };
        let Some(removed) = parts.next() else {
            continue;
        };
        if let Ok(value) = added.parse::<u64>() {
            insertions += value;
        }
        if let Ok(value) = removed.parse::<u64>() {
            deletions += value;
        }
    }

    (Some(insertions), Some(deletions))
}

fn parse_status_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let path = line.get(3..)?.trim();
            if path.is_empty() {
                return None;
            }
            Some(
                path.rsplit_once(" -> ")
                    .map_or(path, |(_, renamed_path)| renamed_path)
                    .to_string(),
            )
        })
        .collect()
}

fn count_paths_by_extension(paths: &[String]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for path in paths {
        let extension = Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        *counts.entry(extension).or_insert(0) += 1;
    }
    counts
}

async fn run_git_capture(repo_root: &Path, args: &[&str]) -> Option<String> {
    let command = async {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout).ok()
    };

    timeout(LOCAL_TELEMETRY_GIT_COMMAND_TIMEOUT, command)
        .await
        .ok()
        .flatten()
}
