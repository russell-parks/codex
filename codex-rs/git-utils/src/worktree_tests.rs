use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::GitWorktreeBaseRef;
use super::WorktreePrepareOptions;
use super::WorktreeStatus;
use super::delete_branch;
use super::prepare_worktree;
use super::remove_worktree;
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
    assert_eq!(prepared.branch_created, true);
    assert_eq!(prepared.generated_name, false);
    assert_eq!(
        git_stdout(prepared.path.as_path(), ["branch", "--show-current"])?,
        prepared.branch
    );

    Ok(())
}

#[test]
fn prepare_worktree_keeps_source_status_clean() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;

    let _prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "clean-source".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;

    assert_eq!(git_stdout(repo.path(), ["status", "--porcelain"])?, "");

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
    assert_eq!(created.branch_created, true);
    assert_eq!(reused.created, false);
    assert_eq!(reused.branch_created, false);
    assert_eq!(reused.path, created.path);
    assert_eq!(reused.branch, "codex-worktree-reuse");
    assert_eq!(reused.generated_name, true);

    Ok(())
}

#[test]
fn creates_worktree_from_existing_branch_without_taking_branch_ownership() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    git_status(repo.path(), ["branch", "codex-worktree-existing"])?;

    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "existing".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;

    assert_eq!(prepared.created, true);
    assert_eq!(prepared.branch_created, false);
    assert_eq!(
        git_stdout(prepared.path.as_path(), ["branch", "--show-current"])?,
        "codex-worktree-existing"
    );

    Ok(())
}

#[test]
fn rejects_existing_path_that_is_unrelated_git_repo() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let target = repo.path().join(".codex").join("worktrees").join("reuse");
    std::fs::create_dir_all(target.as_path())?;
    git_status(target.as_path(), ["init", "-b", "main"])?;

    let err = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "reuse".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )
    .expect_err("unrelated nested repo should not be reused");

    assert!(err.to_string().contains("not part of source repository"));

    Ok(())
}

#[test]
fn rejects_existing_worktree_on_unexpected_branch() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let target = repo
        .path()
        .join(".codex")
        .join("worktrees")
        .join("wrong-branch");
    git_status_os(
        repo.path(),
        vec![
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "other-branch".into(),
            target.as_os_str().to_os_string(),
            "HEAD".into(),
        ],
    )?;

    let err = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "wrong-branch".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )
    .expect_err("worktree on another branch should not be reused");

    assert!(err.to_string().contains("expected branch"));

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
fn rejects_name_that_cannot_be_used_as_branch_ref() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;

    let err = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "bad name".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )
    .expect_err("invalid branch ref should be rejected before creation");

    assert!(err.to_string().contains("unsafe git worktree branch"));
    assert!(!repo.path().join(".codex").join("worktrees").exists());

    Ok(())
}

#[test]
fn fresh_uses_origin_head_when_available() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let remote = tempfile::tempdir()?;
    git_status(remote.path(), ["init", "--bare", "-b", "main"])?;
    git_status_os(
        repo.path(),
        vec![
            "remote".into(),
            "add".into(),
            "origin".into(),
            remote.path().as_os_str().to_os_string(),
        ],
    )?;
    git_status(repo.path(), ["push", "-u", "origin", "main"])?;
    git_status(repo.path(), ["fetch", "origin"])?;
    git_status(
        repo.path(),
        [
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )?;
    git_status(repo.path(), ["checkout", "-b", "feature"])?;
    std::fs::write(repo.path().join("file.txt"), "feature\n")?;
    git_status(repo.path(), ["commit", "-am", "feature"])?;

    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "fresh".to_string(),
            base_ref: GitWorktreeBaseRef::Fresh,
            generated_name: false,
        },
    )?;

    assert_eq!(prepared.base_ref, "origin/HEAD");
    assert_eq!(
        git_stdout(prepared.path.as_path(), ["rev-parse", "HEAD"])?,
        git_stdout(repo.path(), ["rev-parse", "origin/HEAD"])?
    );

    Ok(())
}

#[test]
fn fresh_falls_back_to_head_without_origin_head() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;

    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "fresh-fallback".to_string(),
            base_ref: GitWorktreeBaseRef::Fresh,
            generated_name: false,
        },
    )?;

    assert_eq!(prepared.base_ref, "HEAD");
    assert_eq!(
        git_stdout(prepared.path.as_path(), ["rev-parse", "HEAD"])?,
        git_stdout(repo.path(), ["rev-parse", "HEAD"])?
    );

    Ok(())
}

#[test]
fn recreates_missing_worktree_from_existing_unused_branch() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let options = WorktreePrepareOptions {
        name: "recreate".to_string(),
        base_ref: GitWorktreeBaseRef::Head,
        generated_name: false,
    };
    let first = prepare_worktree(repo.path(), options.clone())?;
    git_status_os(
        repo.path(),
        vec![
            "worktree".into(),
            "remove".into(),
            "--force".into(),
            first.path.as_os_str().to_os_string(),
        ],
    )?;

    let recreated = prepare_worktree(repo.path(), options)?;

    assert_eq!(recreated.created, true);
    assert_eq!(recreated.branch_created, false);
    assert_eq!(recreated.branch, "codex-worktree-recreate");
    assert_eq!(
        git_stdout(recreated.path.as_path(), ["branch", "--show-current"])?,
        "codex-worktree-recreate"
    );

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

#[test]
fn removes_clean_worktree() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "remove-clean".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;

    remove_worktree(repo.path(), prepared.path.as_path())?;

    assert!(!prepared.path.exists());
    assert_eq!(
        git_stdout(repo.path(), ["branch", "--list", &prepared.branch])?,
        prepared.branch
    );

    Ok(())
}

#[test]
fn removing_dirty_worktree_fails_without_force() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "remove-dirty".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;
    std::fs::write(prepared.path.join("dirty.txt"), "dirty\n")?;

    let err = remove_worktree(repo.path(), prepared.path.as_path())
        .expect_err("dirty worktree should not be removed without --force");

    assert!(error_chain_contains(
        &err,
        "contains modified or untracked files"
    ));
    assert!(prepared.path.exists());

    Ok(())
}

#[test]
fn delete_branch_reports_unmerged_branch_error() -> anyhow::Result<()> {
    let repo = TestRepo::new()?;
    let prepared = prepare_worktree(
        repo.path(),
        WorktreePrepareOptions {
            name: "unmerged".to_string(),
            base_ref: GitWorktreeBaseRef::Head,
            generated_name: false,
        },
    )?;
    std::fs::write(prepared.path.join("feature.txt"), "feature\n")?;
    git_status(prepared.path.as_path(), ["add", "."])?;
    git_status(prepared.path.as_path(), ["commit", "-m", "feature"])?;
    git_status_os(
        repo.path(),
        vec![
            "worktree".into(),
            "remove".into(),
            prepared.path.as_os_str().to_os_string(),
        ],
    )?;

    let err = delete_branch(repo.path(), &prepared.branch)
        .expect_err("unmerged branch should not be deleted with -d");

    assert!(error_chain_contains(&err, "not fully merged"));

    Ok(())
}

struct TestRepo {
    _temp_dir: TempDir,
    root: std::path::PathBuf,
}

impl TestRepo {
    fn new() -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path().canonicalize()?;
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

fn error_chain_contains(err: &anyhow::Error, expected: &str) -> bool {
    format!("{err:#}").contains(expected)
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

fn git_status_os(cwd: &Path, args: Vec<std::ffi::OsString>) -> anyhow::Result<()> {
    git_status(cwd, args)
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
