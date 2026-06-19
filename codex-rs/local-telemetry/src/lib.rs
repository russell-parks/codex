pub mod paths;
pub mod privacy;
pub mod reader;
pub mod rollup;
pub mod schema;
pub mod summary;
pub mod writer;

pub use paths::event_file_path;
pub use paths::rollup_file_path;
pub use paths::summary_file_path;
pub use privacy::maybe_hash_prompt;
pub use privacy::maybe_store_prompt;
pub use reader::LocalTelemetryStore;
pub use reader::PruneResult;
pub use rollup::DailyRollup;
pub use rollup::RollupBucket;
pub use schema::TELEMETRY_SCHEMA_VERSION;
pub use schema::TelemetryEvent;
pub use schema::TelemetryEventType;
pub use summary::ApprovalSummary;
pub use summary::ChangedFilesSummary;
pub use summary::ConfigSnapshotSummary;
pub use summary::ConfigSourceSummary;
pub use summary::ErrorSummary;
pub use summary::GitSummary;
pub use summary::PromptMetadataSummary;
pub use summary::SessionSummary;
pub use summary::ToolSummary;
pub use summary::TurnCounts;
pub use summary::UsageTotals;
pub use writer::JsonlTelemetryWriter;
pub use writer::LocalTelemetryWriter;
pub use writer::NoopTelemetryWriter;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
