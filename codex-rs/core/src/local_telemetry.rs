use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use codex_extension_api::ExtensionData;
use codex_local_telemetry::JsonlTelemetryWriter;
use codex_local_telemetry::LocalTelemetryWriter;
use codex_local_telemetry_extension::SessionTelemetryBootstrap;

use crate::config::Config;
use crate::session::session::Session;
use crate::session::session::SessionConfiguration;

pub(crate) fn initialize_session_extension_data(
    config: &Config,
    session_configuration: &SessionConfiguration,
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
    let bootstrap = SessionTelemetryBootstrap {
        invocation_mode: session_configuration.session_source.to_string(),
        cwd: session_configuration.cwd().display().to_string(),
        rollout_path: rollout_path.map(path_to_string),
        model: session_configuration.collaboration_mode.model().to_string(),
        reasoning_effort: session_configuration
            .collaboration_mode
            .reasoning_effort()
            .map(|value| value.to_string()),
        approval_policy: session_configuration.approval_policy.value().to_string(),
        sandbox_mode: format!("{:?}", session_configuration.sandbox_policy()),
        active_profile: config.profile.clone(),
        log_user_prompt: config.telemetry.local.log_user_prompt,
        hash_prompts: config.telemetry.local.hash_prompts,
        write_run_summary: config.telemetry.local.write_run_summary,
    };
    codex_local_telemetry_extension::initialize_session_data(
        session_store,
        writer,
        raw_event_path,
        bootstrap,
    );
}

pub(crate) async fn update_session_stop_metadata(session: &Session) {
    let rollout_path = match session.current_rollout_path().await {
        Ok(path) => path.map(path_to_string),
        Err(err) => {
            tracing::warn!("failed to read local telemetry rollout path at shutdown: {err}");
            None
        }
    };
    codex_local_telemetry_extension::update_session_stop_metadata(
        &session.services.session_extension_data,
        rollout_path,
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
