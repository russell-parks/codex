mod extension;
mod state;

pub use extension::SessionTelemetryBootstrap;
pub use extension::initialize_session_data;
pub use extension::install;
pub use extension::record_approval_requested;
pub use extension::record_approval_resolved;
pub use extension::record_rate_limits;
pub use extension::record_task_type;
pub use extension::record_turn_profile;
pub use extension::record_user_prompt;
pub use extension::update_session_stop_metadata;
pub use extension::update_session_stop_metadata_with_details;
pub use extension::update_session_stop_metadata_with_git;
pub use state::SessionStopUpdate;

#[cfg(test)]
#[path = "extension_tests.rs"]
mod extension_tests;
