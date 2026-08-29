use std::path::Path;
use std::path::PathBuf;

use codex_git_utils::worktree::PreparedWorktree;

#[derive(Debug, thiserror::Error)]
#[error(
    "failed to initialize sqlite local db at {}: {detail}",
    database_path.display()
)]
pub struct LocalStateDbStartupError {
    database_path: PathBuf,
    detail: String,
    worktree_cleanup: Option<PreparedWorktree>,
}

impl LocalStateDbStartupError {
    pub fn new(database_path: PathBuf, detail: String) -> Self {
        Self {
            database_path,
            detail,
            worktree_cleanup: None,
        }
    }

    pub fn with_worktree_cleanup(mut self, worktree_cleanup: Option<PreparedWorktree>) -> Self {
        self.worktree_cleanup = worktree_cleanup;
        self
    }

    pub fn database_path(&self) -> &Path {
        self.database_path.as_path()
    }

    pub fn state_db_path(&self) -> &Path {
        self.database_path()
    }

    pub fn detail(&self) -> &str {
        self.detail.as_str()
    }

    pub fn worktree_cleanup(&self) -> Option<&PreparedWorktree> {
        self.worktree_cleanup.as_ref()
    }
}
