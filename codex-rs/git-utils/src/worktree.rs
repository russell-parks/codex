use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;

use crate::GitToolingError;
use crate::operations::resolve_repository_root;
use crate::operations::run_git_for_status;
use crate::operations::run_git_for_stdout;

/// Base revision used when creating a Codex-owned Git worktree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitWorktreeBaseRef {
    /// Prefer the repository's remote default branch, falling back to `HEAD`.
    Fresh,
    /// Create the worktree from the current `HEAD`.
    Head,
}

/// Options for preparing a Codex-owned Git worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreePrepareOptions {
    pub name: String,
    pub base_ref: GitWorktreeBaseRef,
    pub generated_name: bool,
}

/// Metadata for a prepared Codex-owned Git worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedWorktree {
    pub source_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base_ref: String,
    pub created: bool,
    pub generated_name: bool,
}

/// Cleanliness state for a worktree checkout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeStatus {
    Clean,
    Dirty,
    Missing,
}

/// Create or reuse a Codex-owned Git worktree.
///
/// # Errors
///
/// Returns an error when `repo_hint` is not inside a Git repository, the
/// requested name is unsafe for a local worktree path, Git fails to create the
/// worktree, or the target path already exists but is not a Git worktree root.
pub fn prepare_worktree(
    repo_hint: &Path,
    options: WorktreePrepareOptions,
) -> Result<PreparedWorktree> {
    validate_worktree_name(&options.name)?;

    let source_root = resolve_repository_root(repo_hint)
        .with_context(|| format!("failed to resolve git repository root from {repo_hint:?}"))?;
    let path = source_root
        .join(".codex")
        .join("worktrees")
        .join(&options.name);
    let branch = format!("codex-worktree-{}", options.name);
    let base_ref = resolve_base_ref(source_root.as_path(), options.base_ref)?;

    if path.try_exists()? {
        if is_git_worktree_root(path.as_path())? {
            return Ok(PreparedWorktree {
                source_root,
                path,
                branch,
                base_ref,
                created: false,
                generated_name: options.generated_name,
            });
        }

        bail!("worktree path {path:?} already exists but is not a git worktree");
    }

    let worktrees_root = source_root.join(".codex").join("worktrees");
    std::fs::create_dir_all(worktrees_root.as_path())
        .with_context(|| format!("failed to create codex worktree directory {worktrees_root:?}"))?;

    run_git_for_status(
        source_root.as_path(),
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(&branch),
            path.as_os_str().to_os_string(),
            OsString::from(&base_ref),
        ],
        /*env*/ None,
    )
    .with_context(|| format!("failed to create git worktree at {path:?}"))?;

    Ok(PreparedWorktree {
        source_root,
        path,
        branch,
        base_ref,
        created: true,
        generated_name: options.generated_name,
    })
}

/// Return the cleanliness state of an existing worktree checkout.
///
/// # Errors
///
/// Returns an error when `path` exists but Git cannot inspect it as a worktree.
pub fn worktree_status(path: &Path) -> Result<WorktreeStatus> {
    if !path.try_exists()? {
        return Ok(WorktreeStatus::Missing);
    }

    let status = run_git_for_stdout(
        path,
        [OsString::from("status"), OsString::from("--porcelain")],
        /*env*/ None,
    )
    .with_context(|| format!("failed to inspect git worktree status at {path:?}"))?;

    if status.is_empty() {
        Ok(WorktreeStatus::Clean)
    } else {
        Ok(WorktreeStatus::Dirty)
    }
}

fn resolve_base_ref(source_root: &Path, base_ref: GitWorktreeBaseRef) -> Result<String> {
    match base_ref {
        GitWorktreeBaseRef::Fresh if git_ref_exists(source_root, "origin/HEAD")? => {
            Ok("origin/HEAD".to_string())
        }
        GitWorktreeBaseRef::Fresh | GitWorktreeBaseRef::Head => Ok("HEAD".to_string()),
    }
}

fn git_ref_exists(source_root: &Path, ref_name: &str) -> Result<bool> {
    match run_git_for_status(
        source_root,
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(ref_name),
        ],
        /*env*/ None,
    ) {
        Ok(()) => Ok(true),
        Err(GitToolingError::GitCommand { .. }) => Ok(false),
        Err(err) => Err(err).with_context(|| format!("failed to resolve git ref {ref_name}")),
    }
}

fn is_git_worktree_root(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }

    match resolve_repository_root(path) {
        Ok(root) => paths_equal(root.as_path(), path),
        Err(GitToolingError::GitCommand { .. })
        | Err(GitToolingError::NotAGitRepository { .. }) => Ok(false),
        Err(err) => Err(err).with_context(|| format!("failed to inspect git worktree {path:?}")),
    }
}

fn paths_equal(left: &Path, right: &Path) -> Result<bool> {
    let left = left
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path {left:?}"))?;
    let right = right
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path {right:?}"))?;
    Ok(left == right)
}

fn validate_worktree_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        bail!("unsafe git worktree name {name:?}");
    }

    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component == OsStr::new(name) => Ok(()),
        _ => bail!("unsafe git worktree name {name:?}"),
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
