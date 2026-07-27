use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::GitWorktreeBaseRef;
use super::WorktreePrepareOptions;
use super::WorktreeStatus;
use super::prepare_worktree;
use super::worktree_status;

#[test]
fn creates_worktree_from_head() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;

    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "task-2".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;

    assert_eq!(prepared.source_root.as_path(), repo.path());
    assert_eq!(
        prepared.path,
        repo.path().join(".codex").join("worktrees").join("task-2")
    );
    assert_eq!(prepared.branch, "codex-worktree-task-2");
    assert_eq!(prepared.base_ref, "HEAD");
    assert_eq!(prepared.created, true);
    assert_eq!(prepared.generated_name, false);
    assert_eq!(
        git_stdout(prepared.path.as_path(), ["branch", "--show-current"])?,
        prepared.branch
    );

    Ok(())
}

#[test]
fn reuses_existing_git_worktree() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let options = WorktreePrepareOptions {
        name: "reuse".to_string(),
        base_ref: GitWorktreeBaseRef::Head,
        generated_name: true,
    };

    let created = prepare_worktree(repo.path(), options.clone())?;
    let reused = prepare_worktree(repo.path(), options)?;

    assert_eq!(created.created, true);
    assert_eq!(reused.created, false);
    assert_eq!(reused.path, created.path);
    assert_eq!(reused.branch, "codex-worktree-reuse");
    assert_eq!(reused.generated_name, true);

    Ok(())
}

#[test]
fn rejects_unsafe_worktree_name() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;

    let err = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "../escape".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )
    .expect_err("unsafe name should be rejected");

    assert!(err.to_string().contains("unsafe git worktree name"));
    assert!(!repo.path().join(".codex").join("worktrees").exists());

    Ok(())
}

#[test]
fn reports_worktree_status() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "status".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;

    assert_eq!(
        worktree_status(prepared.path.as_path())?,
        WorktreeStatus::Clean
    );
    std::fs::write(prepared.path.join("file.txt"), "dirty\n")?;
    assert_eq!(
        worktree_status(prepared.path.as_path())?,
        WorktreeStatus::Dirty
    );
    assert_eq!(
        worktree_status(
            repo.path()
                .join(".codex")
                .join("worktrees")
                .join("missing")
                .as_path()
        )?,
        WorktreeStatus::Missing
    );

    Ok(())
}

struct TestRepo {
    _temp_dir: TempDir,
    root: std::path::PathBuf,
}

impl TestRepo {
    fn new() -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path().to_path_buf();
        git_status(root.as_path(), ["init", "-b", "main"])?;
        git_status(
            root.as_path(),
            ["config", "user.email", "codex@example.com"],
        )?;
        git_status(root.as_path(), ["config", "user.name", "Codex Test"])?;
        std::fs::write(root.join("file.txt"), "initial\n")?;
        git_status(root.as_path(), ["add", "."])?;
        git_status(root.as_path(), ["commit", "-m", "initial"])?;

        Ok(Self {
            _temp_dir: temp_dir,
            root,
        })
    }

    fn path(&self) -> &Path {
        self.root.as_path()
    }
}

fn git_status<I, S>(cwd: &Path, args: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn git_stdout<I, S>(cwd: &Path, args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
