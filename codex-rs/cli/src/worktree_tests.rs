use std::io;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use codex_git_utils::worktree::PreparedWorktree;
use pretty_assertions::assert_eq;

use super::CleanupRunner;
use super::cleanup_worktree_on_exit_with;
use super::parse_cleanup_confirmation;

#[test]
fn cleanup_confirmation_accepts_yes_only() {
    assert!(parse_cleanup_confirmation("y"));
    assert!(parse_cleanup_confirmation("Y"));
    assert!(parse_cleanup_confirmation("yes"));
    assert!(parse_cleanup_confirmation("YES"));
    assert!(!parse_cleanup_confirmation(""));
    assert!(!parse_cleanup_confirmation("\n"));
    assert!(!parse_cleanup_confirmation("n"));
    assert!(!parse_cleanup_confirmation("no"));
    assert!(!parse_cleanup_confirmation("sure"));
}

#[test]
fn cleanup_skips_reused_worktree_without_prompting_or_removing() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ false, /*branch_created*/ false);
    let mut input = io::Cursor::new("y\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner::default();

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(String::from_utf8(output).unwrap(), "");
    assert_eq!(runner.removed, Vec::<PathBuf>::new());
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn cleanup_skips_created_worktree_when_stdin_is_not_terminal() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("y\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner::default();

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ false,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(String::from_utf8(output).unwrap(), "");
    assert_eq!(runner.removed, Vec::<PathBuf>::new());
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn cleanup_skips_created_worktree_when_stderr_is_not_terminal() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("y\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner::default();

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ false,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(String::from_utf8(output).unwrap(), "");
    assert_eq!(runner.removed, Vec::<PathBuf>::new());
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn cleanup_keeps_created_worktree_by_default() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner::default();

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert!(String::from_utf8(output).unwrap().contains("[y/N]"));
    assert_eq!(runner.removed, Vec::<PathBuf>::new());
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn cleanup_preserves_unowned_existing_branch_after_remove() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ false);
    let mut input = io::Cursor::new("yes\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner::default();

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(runner.removed, vec![worktree.path.clone()]);
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn branch_delete_failure_is_nonfatal_after_remove() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("yes\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner {
        delete_error: Some(anyhow::anyhow!("not fully merged")),
        ..Default::default()
    };

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(runner.removed, vec![worktree.path.clone()]);
    assert_eq!(runner.deleted, vec![worktree.branch.clone()]);
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("failed to delete branch codex-worktree-task")
    );

    Ok(())
}

#[test]
fn cleanup_warning_includes_wrapped_remove_error_source() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("y\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner {
        remove_error: Some(wrapped_git_error()),
        ..Default::default()
    };

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("failed to remove git worktree"));
    assert!(output.contains("contains modified or untracked files"));
    assert_eq!(runner.removed, vec![worktree.path.clone()]);
    assert_eq!(runner.deleted, Vec::<String>::new());

    Ok(())
}

#[test]
fn dirty_worktree_remove_failure_is_nonfatal_and_does_not_delete_branch() -> io::Result<()> {
    let worktree = prepared_worktree(/*created*/ true, /*branch_created*/ true);
    let mut input = io::Cursor::new("y\n");
    let mut output = Vec::new();
    let mut runner = FakeCleanupRunner {
        remove_error: Some(anyhow::anyhow!("contains modified or untracked files")),
        ..Default::default()
    };

    cleanup_worktree_on_exit_with(
        Some(&worktree),
        /*stdin_is_terminal*/ true,
        /*stderr_is_terminal*/ true,
        &mut input,
        &mut output,
        &mut runner,
    )?;

    assert_eq!(runner.removed, vec![worktree.path.clone()]);
    assert_eq!(runner.deleted, Vec::<String>::new());
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("failed to remove Codex worktree")
    );

    Ok(())
}

fn wrapped_git_error() -> anyhow::Error {
    Err::<(), _>(anyhow::anyhow!("contains modified or untracked files"))
        .context("failed to remove git worktree at \"/repo/.codex/worktrees/task\"")
        .unwrap_err()
}

fn prepared_worktree(created: bool, branch_created: bool) -> PreparedWorktree {
    PreparedWorktree {
        source_root: Path::new("/repo").to_path_buf(),
        path: Path::new("/repo/.codex/worktrees/task").to_path_buf(),
        branch: "codex-worktree-task".to_string(),
        base_ref: "HEAD".to_string(),
        created,
        branch_created,
        generated_name: false,
    }
}

#[derive(Default)]
struct FakeCleanupRunner {
    removed: Vec<PathBuf>,
    deleted: Vec<String>,
    remove_error: Option<anyhow::Error>,
    delete_error: Option<anyhow::Error>,
}

impl CleanupRunner for FakeCleanupRunner {
    fn remove_worktree(&mut self, worktree: &PreparedWorktree) -> Result<()> {
        self.removed.push(worktree.path.clone());
        if let Some(err) = self.remove_error.take() {
            return Err(err);
        }
        Ok(())
    }

    fn delete_branch(&mut self, worktree: &PreparedWorktree) -> Result<()> {
        self.deleted.push(worktree.branch.clone());
        if let Some(err) = self.delete_error.take() {
            return Err(err);
        }
        Ok(())
    }
}
