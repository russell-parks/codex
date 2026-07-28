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

const CODEX_WORKTREES_EXCLUDE: &str = ".codex/worktrees/";

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
    pub branch_created: bool,
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
    validate_branch_name(source_root.as_path(), &branch, &options.name)?;
    let base_ref = resolve_base_ref(source_root.as_path(), options.base_ref)?;
    ensure_codex_worktrees_excluded(source_root.as_path())?;

    if path.try_exists()? {
        validate_existing_worktree(source_root.as_path(), path.as_path(), &branch)?;
        return Ok(PreparedWorktree {
            source_root,
            path,
            branch,
            base_ref,
            created: false,
            branch_created: false,
            generated_name: options.generated_name,
        });
    }

    let worktrees_root = source_root.join(".codex").join("worktrees");
    std::fs::create_dir_all(worktrees_root.as_path())
        .with_context(|| format!("failed to create codex worktree directory {worktrees_root:?}"))?;

    let branch_exists = local_branch_exists(source_root.as_path(), &branch)?;
    let args = if branch_exists {
        if let Some(checked_out_path) = branch_checked_out_path(source_root.as_path(), &branch)? {
            bail!(
                "git worktree branch {branch:?} is already checked out at {checked_out_path:?}; run `git worktree prune` or remove that worktree before recreating"
            );
        }

        vec![
            OsString::from("worktree"),
            OsString::from("add"),
            path.as_os_str().to_os_string(),
            OsString::from(&branch),
        ]
    } else {
        vec![
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(&branch),
            path.as_os_str().to_os_string(),
            OsString::from(&base_ref),
        ]
    };

    run_git_for_status(source_root.as_path(), args, /*env*/ None)
        .with_context(|| format!("failed to create git worktree at {path:?}"))?;

    Ok(PreparedWorktree {
        source_root,
        path,
        branch,
        base_ref,
        created: true,
        branch_created: !branch_exists,
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

/// Remove a Git worktree without forcing dirty checkout cleanup.
///
/// # Errors
///
/// Returns an error when Git cannot remove the worktree. Dirty worktrees are
/// expected to fail because this helper intentionally does not pass `--force`.
pub fn remove_worktree(source_root: &Path, worktree_path: &Path) -> Result<()> {
    run_git_for_status(
        source_root,
        [
            OsString::from("worktree"),
            OsString::from("remove"),
            worktree_path.as_os_str().to_os_string(),
        ],
        /*env*/ None,
    )
    .with_context(|| format!("failed to remove git worktree at {worktree_path:?}"))
}

/// Delete a local Git branch with `git branch -d`.
///
/// # Errors
///
/// Returns an error when Git declines to delete the branch, including when the
/// branch is unmerged or checked out by another worktree.
pub fn delete_branch(source_root: &Path, branch: &str) -> Result<()> {
    run_git_for_status(
        source_root,
        [
            OsString::from("branch"),
            OsString::from("-d"),
            OsString::from(branch),
        ],
        /*env*/ None,
    )
    .with_context(|| format!("failed to delete git branch {branch:?}"))
}

fn ensure_codex_worktrees_excluded(source_root: &Path) -> Result<()> {
    let common_dir = canonical_git_common_dir(source_root).with_context(|| {
        format!("failed to inspect source repository common git dir at {source_root:?}")
    })?;
    let info_dir = common_dir.join("info");
    std::fs::create_dir_all(info_dir.as_path())
        .with_context(|| format!("failed to create git info directory {info_dir:?}"))?;
    let exclude_path = info_dir.join("exclude");
    let existing = match std::fs::read_to_string(exclude_path.as_path()) {
        Ok(existing) => existing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read git local exclude file {exclude_path:?}")
            });
        }
    };
    if existing
        .lines()
        .any(|line| line.trim() == CODEX_WORKTREES_EXCLUDE)
    {
        return Ok(());
    }

    let mut exclude_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(exclude_path.as_path())
        .with_context(|| format!("failed to open git local exclude file {exclude_path:?}"))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        use std::io::Write as _;
        writeln!(exclude_file)
            .with_context(|| format!("failed to update git local exclude file {exclude_path:?}"))?;
    }
    {
        use std::io::Write as _;
        writeln!(exclude_file, "{CODEX_WORKTREES_EXCLUDE}")
            .with_context(|| format!("failed to update git local exclude file {exclude_path:?}"))?;
    }

    Ok(())
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

fn validate_branch_name(source_root: &Path, branch: &str, name: &str) -> Result<()> {
    match run_git_for_status(
        source_root,
        [
            OsString::from("check-ref-format"),
            OsString::from("--branch"),
            OsString::from(branch),
        ],
        /*env*/ None,
    ) {
        Ok(()) => Ok(()),
        Err(GitToolingError::GitCommand { .. }) => {
            bail!("unsafe git worktree branch {branch:?} derived from name {name:?}")
        }
        Err(err) => Err(err).with_context(|| format!("failed to validate git branch {branch:?}")),
    }
}

fn validate_existing_worktree(
    source_root: &Path,
    path: &Path,
    expected_branch: &str,
) -> Result<()> {
    if !path.is_dir() {
        bail!("worktree path {path:?} already exists but is not a git worktree");
    }

    match resolve_repository_root(path) {
        Ok(root) => {
            if !paths_equal(root.as_path(), path)? {
                bail!("worktree path {path:?} already exists but is not a git worktree root");
            }
        }
        Err(GitToolingError::GitCommand { .. })
        | Err(GitToolingError::NotAGitRepository { .. }) => {
            bail!("worktree path {path:?} already exists but is not a git worktree")
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to inspect git worktree {path:?}"));
        }
    }

    let source_common_dir = canonical_git_common_dir(source_root).with_context(|| {
        format!("failed to inspect source repository common git dir at {source_root:?}")
    })?;
    let path_common_dir = canonical_git_common_dir(path)
        .with_context(|| format!("failed to inspect worktree common git dir at {path:?}"))?;
    if source_common_dir != path_common_dir {
        bail!("worktree path {path:?} is not part of source repository {source_root:?}");
    }

    let branch = current_branch(path)
        .with_context(|| format!("failed to inspect branch for existing worktree {path:?}"))?;
    if branch != expected_branch {
        bail!(
            "worktree path {path:?} is on branch {branch:?}; expected branch {expected_branch:?}"
        );
    }

    Ok(())
}

fn canonical_git_common_dir(path: &Path) -> Result<PathBuf> {
    let common_dir = run_git_for_stdout(
        path,
        [
            OsString::from("rev-parse"),
            OsString::from("--git-common-dir"),
        ],
        /*env*/ None,
    )?;
    let common_dir = PathBuf::from(common_dir);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        path.join(common_dir)
    };
    common_dir
        .canonicalize()
        .with_context(|| format!("failed to canonicalize git common dir {common_dir:?}"))
}

fn current_branch(path: &Path) -> Result<String> {
    run_git_for_stdout(
        path,
        [
            OsString::from("symbolic-ref"),
            OsString::from("--quiet"),
            OsString::from("--short"),
            OsString::from("HEAD"),
        ],
        /*env*/ None,
    )
    .map_err(anyhow::Error::from)
}

fn local_branch_exists(source_root: &Path, branch: &str) -> Result<bool> {
    match run_git_for_status(
        source_root,
        [
            OsString::from("show-ref"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(format!("refs/heads/{branch}")),
        ],
        /*env*/ None,
    ) {
        Ok(()) => Ok(true),
        Err(GitToolingError::GitCommand { .. }) => Ok(false),
        Err(err) => Err(err).with_context(|| format!("failed to inspect git branch {branch:?}")),
    }
}

fn branch_checked_out_path(source_root: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let expected_ref = format!("refs/heads/{branch}");
    let expected_branch_line = format!("branch {expected_ref}");
    let worktrees = run_git_for_stdout(
        source_root,
        [
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
        ],
        /*env*/ None,
    )
    .context("failed to list git worktrees")?;

    let mut current_path: Option<PathBuf> = None;
    for line in worktrees.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path));
        } else if line == expected_branch_line {
            return Ok(current_path);
        } else if line.is_empty() {
            current_path = None;
        }
    }

    Ok(None)
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
