use std::io;
use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Write;

use anyhow::Result;
use codex_git_utils::worktree::PreparedWorktree;
use codex_git_utils::worktree::delete_branch;
use codex_git_utils::worktree::remove_worktree;
use codex_tui::LocalStateDbStartupError;

pub(crate) fn parse_cleanup_confirmation(input: &str) -> bool {
    let answer = input.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

pub(crate) fn should_prompt_cleanup(
    worktree: &PreparedWorktree,
    stdin_is_terminal: bool,
    stderr_is_terminal: bool,
) -> bool {
    worktree.created && stdin_is_terminal && stderr_is_terminal
}

pub(crate) fn startup_retry_cleanup_worktree(
    startup_error: &LocalStateDbStartupError,
) -> Option<&PreparedWorktree> {
    startup_error
        .worktree_cleanup()
        .filter(|worktree| worktree.created)
}

pub(crate) trait CleanupRunner {
    fn remove_worktree(&mut self, worktree: &PreparedWorktree) -> Result<()>;
    fn delete_branch(&mut self, worktree: &PreparedWorktree) -> Result<()>;
}

pub(crate) fn cleanup_worktree_on_exit(worktree: Option<&PreparedWorktree>) {
    let stdin = io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let stderr_is_terminal = stderr.is_terminal();
    let mut output = stderr.lock();
    let mut runner = GitCleanupRunner;
    if let Err(err) = cleanup_worktree_on_exit_with(
        worktree,
        stdin_is_terminal,
        stderr_is_terminal,
        &mut input,
        &mut output,
        &mut runner,
    ) {
        eprintln!("WARNING: failed to run Codex worktree cleanup prompt: {err:#}");
    }
}

pub(crate) fn cleanup_worktree_on_exit_with<R, I, W>(
    worktree: Option<&PreparedWorktree>,
    stdin_is_terminal: bool,
    stderr_is_terminal: bool,
    input: &mut I,
    output: &mut W,
    runner: &mut R,
) -> io::Result<()>
where
    R: CleanupRunner,
    I: BufRead,
    W: Write,
{
    let Some(worktree) = worktree
        .filter(|worktree| should_prompt_cleanup(worktree, stdin_is_terminal, stderr_is_terminal))
    else {
        return Ok(());
    };

    write!(
        output,
        "Remove Codex worktree at {}? This can discard uncommitted work. [y/N]: ",
        worktree.path.display()
    )?;
    output.flush()?;

    let mut answer = String::new();
    input.read_line(&mut answer)?;
    if !parse_cleanup_confirmation(&answer) {
        return Ok(());
    }

    if let Err(err) = runner.remove_worktree(worktree) {
        writeln!(
            output,
            "WARNING: failed to remove Codex worktree at {}: {err:#}",
            worktree.path.display()
        )?;
        return Ok(());
    }

    if !worktree.branch_created {
        return Ok(());
    }

    if let Err(err) = runner.delete_branch(worktree) {
        writeln!(
            output,
            "WARNING: removed Codex worktree at {}, but failed to delete branch {}: {err:#}",
            worktree.path.display(),
            worktree.branch
        )?;
    }

    Ok(())
}

struct GitCleanupRunner;

impl CleanupRunner for GitCleanupRunner {
    fn remove_worktree(&mut self, worktree: &PreparedWorktree) -> Result<()> {
        remove_worktree(worktree.source_root.as_path(), worktree.path.as_path())
    }

    fn delete_branch(&mut self, worktree: &PreparedWorktree) -> Result<()> {
        delete_branch(worktree.source_root.as_path(), &worktree.branch)
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
