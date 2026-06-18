mod extension;
mod state;

pub use extension::SessionTelemetryBootstrap;
pub use extension::initialize_session_data;
pub use extension::install;
pub use extension::record_user_prompt;
pub use extension::update_session_stop_metadata;

#[cfg(test)]
#[path = "extension_tests.rs"]
mod extension_tests;
