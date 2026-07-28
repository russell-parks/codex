use std::io;
use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Write;

use anyhow::Result;
use codex_git_utils::worktree::PreparedWorktree;
use codex_git_utils::worktree::delete_branch;
use codex_git_utils::worktree::remove_worktree;

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
        eprintln!("WARNING: failed to run Codex worktree cleanup prompt: {err}");
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
            "WARNING: failed to remove Codex worktree at {}: {err}",
            worktree.path.display()
        )?;
        return Ok(());
    }

    if let Err(err) = runner.delete_branch(worktree) {
        writeln!(
            output,
            "WARNING: removed Codex worktree at {}, but failed to delete branch {}: {err}",
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
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;

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
        let worktree = prepared_worktree(/*created*/ false);
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
        let worktree = prepared_worktree(/*created*/ true);
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
        let worktree = prepared_worktree(/*created*/ true);
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
        let worktree = prepared_worktree(/*created*/ true);
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
    fn branch_delete_failure_is_nonfatal_after_remove() -> io::Result<()> {
        let worktree = prepared_worktree(/*created*/ true);
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
    fn dirty_worktree_remove_failure_is_nonfatal_and_does_not_delete_branch() -> io::Result<()> {
        let worktree = prepared_worktree(/*created*/ true);
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

    fn prepared_worktree(created: bool) -> PreparedWorktree {
        PreparedWorktree {
            source_root: Path::new("/repo").to_path_buf(),
            path: Path::new("/repo/.codex/worktrees/task").to_path_buf(),
            branch: "codex-worktree-task".to_string(),
            base_ref: "HEAD".to_string(),
            created,
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
}
