use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use codex_extension_api::ExtensionData;
use codex_git_utils::canonicalize_git_remote_url;
use codex_git_utils::current_branch_name;
use codex_git_utils::get_git_remote_urls_assume_git_repo;
use codex_git_utils::get_git_repo_root;
use codex_git_utils::get_has_changes;
use codex_git_utils::get_head_commit_hash;
use codex_local_telemetry::GitSummary;
use codex_local_telemetry::JsonlTelemetryWriter;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry_extension::SessionTelemetryBootstrap;
use codex_protocol::protocol::InitialHistory;

use crate::ThreadConfigSnapshot;
use crate::config::Config;
use crate::session::session::Session;

pub(crate) async fn initialize_session_extension_data(
    config: &Config,
    thread_config: &ThreadConfigSnapshot,
    initial_history: &InitialHistory,
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
        log_user_prompt: config.telemetry.local.log_user_prompt,
        hash_prompts: config.telemetry.local.hash_prompts,
        write_run_summary: config.telemetry.local.write_run_summary,
        capture_session: config.telemetry.local.capture_session,
        capture_turns: config.telemetry.local.capture_turns,
        capture_usage: config.telemetry.local.capture_usage,
        capture_tool_calls: config.telemetry.local.capture_tool_calls,
        capture_errors: config.telemetry.local.capture_errors,
    };
    codex_local_telemetry_extension::initialize_session_data(
        session_store,
        writer,
        raw_event_path,
        bootstrap,
    );
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
    codex_local_telemetry_extension::update_session_stop_metadata_with_git(
        &session.services.session_extension_data,
        rollout_path,
        git,
    );
}

pub(crate) fn record_user_prompt(session_store: &ExtensionData, turn_id: &str, prompt_text: &str) {
    codex_local_telemetry_extension::record_user_prompt(session_store, turn_id, prompt_text);
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
