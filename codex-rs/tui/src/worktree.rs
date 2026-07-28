use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::AppServerTarget;
use crate::Cli;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_config::types::WorktreeBaseRef;
use codex_git_utils::worktree::GitWorktreeBaseRef;
use codex_git_utils::worktree::PreparedWorktree;
use codex_git_utils::worktree::WorktreePrepareOptions;
use codex_git_utils::worktree::prepare_worktree;
use codex_utils_absolute_path::AbsolutePathBuf;
use uuid::Uuid;

mod worktree_include;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct WorktreeRequest {
    pub(crate) name: String,
    pub(crate) generated_name: bool,
}

impl WorktreeRequest {
    pub(crate) fn from_cli_value(value: Option<&str>) -> Result<Option<Self>> {
        let Some(value) = value else {
            return Ok(None);
        };

        if value.is_empty() {
            return Ok(Some(Self::generated_from_uuid(Uuid::new_v4())));
        }

        validate_user_worktree_name(value)?;
        Ok(Some(Self {
            name: value.to_string(),
            generated_name: false,
        }))
    }

    pub(crate) fn generated_from_uuid(uuid: Uuid) -> Self {
        Self {
            name: format!("session-{}", uuid.simple()),
            generated_name: true,
        }
    }
}

pub(crate) fn prepare_launch_worktree(
    repo_hint: &Path,
    cli_worktree: Option<&str>,
    base_ref: WorktreeBaseRef,
) -> Result<Option<PreparedWorktree>> {
    let Some(request) = WorktreeRequest::from_cli_value(cli_worktree)? else {
        return Ok(None);
    };

    let branch_cleanup =
        branch_cleanup_for_include_failure(repo_hint, &codex_worktree_branch_name(&request.name));
    let prepared_worktree = prepare_worktree(
        repo_hint,
        WorktreePrepareOptions {
            name: request.name,
            base_ref: git_base_ref_from_config(base_ref),
            generated_name: request.generated_name,
        },
    )?;
    if let Err(err) = worktree_include::copy_worktree_include_files(&prepared_worktree) {
        if prepared_worktree.created {
            return Err(cleanup_created_worktree_after_include_failure(
                &prepared_worktree,
                branch_cleanup,
                err,
            ));
        }

        return Err(err);
    }
    Ok(Some(prepared_worktree))
}

pub(crate) fn validate_app_server_target(
    cli_worktree: Option<&str>,
    app_server_target: &AppServerTarget,
) -> std::io::Result<()> {
    if cli_worktree.is_none() {
        return Ok(());
    }

    match app_server_target {
        AppServerTarget::Embedded => Ok(()),
        AppServerTarget::LocalDaemon { .. } | AppServerTarget::Remote { .. } => Err(
            std::io::Error::other("--worktree is not supported when connected to an app server"),
        ),
    }
}

pub(crate) fn repo_hint_for_target<'a>(
    cli_worktree: Option<&str>,
    app_server_target: &AppServerTarget,
    config_cwd: Option<&'a AbsolutePathBuf>,
) -> std::io::Result<Option<&'a Path>> {
    if cli_worktree.is_none() {
        return Ok(None);
    }

    match app_server_target {
        AppServerTarget::Embedded => config_cwd
            .as_ref()
            .map(|cwd| cwd.as_path())
            .ok_or_else(|| std::io::Error::other("--worktree requires a local working directory")),
        AppServerTarget::LocalDaemon { .. } | AppServerTarget::Remote { .. } => Err(
            std::io::Error::other("--worktree is not supported when connected to an app server"),
        ),
    }
    .map(Some)
}

pub(crate) fn final_cwd_override_for_launch(
    uses_remote_workspace: bool,
    cwd: Option<PathBuf>,
    prepared_worktree: Option<&PreparedWorktree>,
) -> Option<PathBuf> {
    if uses_remote_workspace {
        None
    } else {
        prepared_worktree
            .map(|worktree| final_cwd_for_prepared_worktree(cwd.as_deref(), worktree))
            .or(cwd)
    }
}

fn final_cwd_for_prepared_worktree(
    cwd: Option<&Path>,
    prepared_worktree: &PreparedWorktree,
) -> PathBuf {
    let Some(cwd) = cwd else {
        return prepared_worktree.path.clone();
    };
    let relative_cwd = cwd
        .strip_prefix(prepared_worktree.source_root.as_path())
        .unwrap_or_else(|_| Path::new(""));
    if relative_cwd.as_os_str().is_empty() {
        prepared_worktree.path.clone()
    } else {
        prepared_worktree.path.join(relative_cwd)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WorktreeLaunchMode {
    resume_picker: bool,
    resume_last: bool,
    resume_session_id: bool,
    fork_picker: bool,
    fork_last: bool,
    fork_session_id: bool,
}

impl WorktreeLaunchMode {
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        Self {
            resume_picker: cli.resume_picker,
            resume_last: cli.resume_last,
            resume_session_id: cli.resume_session_id.is_some(),
            fork_picker: cli.fork_picker,
            fork_last: cli.fork_last,
            fork_session_id: cli.fork_session_id.is_some(),
        }
    }

    fn is_resume_or_fork(self) -> bool {
        self.resume_picker
            || self.resume_last
            || self.resume_session_id
            || self.fork_picker
            || self.fork_last
            || self.fork_session_id
    }
}

pub(crate) fn validate_starts_new_session(
    cli_worktree: Option<&str>,
    launch_mode: WorktreeLaunchMode,
) -> Result<(), &'static str> {
    if cli_worktree.is_some() && launch_mode.is_resume_or_fork() {
        Err("--worktree is only supported when starting a new interactive session")
    } else {
        Ok(())
    }
}

pub(crate) fn git_base_ref_from_config(base_ref: WorktreeBaseRef) -> GitWorktreeBaseRef {
    match base_ref {
        WorktreeBaseRef::Fresh => GitWorktreeBaseRef::Fresh,
        WorktreeBaseRef::Head => GitWorktreeBaseRef::Head,
    }
}

fn validate_user_worktree_name(name: &str) -> Result<()> {
    if name == "." || name == ".." {
        bail!(
            "worktree name must contain only ASCII letters, digits, '.', '_' or '-', got {name:?}"
        );
    }

    if name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        bail!(
            "worktree name must contain only ASCII letters, digits, '.', '_' or '-', got {name:?}"
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BranchCleanup {
    DeleteCreatedBranch,
    PreserveExistingBranch,
}

fn codex_worktree_branch_name(worktree_name: &str) -> String {
    format!("codex-worktree-{worktree_name}")
}

fn branch_cleanup_for_include_failure(repo_hint: &Path, branch: &str) -> BranchCleanup {
    let output = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(repo_hint)
        .output();
    match output {
        Ok(output) if output.status.success() => BranchCleanup::PreserveExistingBranch,
        Ok(output) if output.status.code() == Some(1) => BranchCleanup::DeleteCreatedBranch,
        Ok(_) | Err(_) => BranchCleanup::PreserveExistingBranch,
    }
}

fn cleanup_created_worktree_after_include_failure(
    prepared_worktree: &PreparedWorktree,
    branch_cleanup: BranchCleanup,
    include_error: anyhow::Error,
) -> anyhow::Error {
    match cleanup_created_worktree_after_include_failure_inner(prepared_worktree, branch_cleanup) {
        Ok(()) => include_error.context(format!(
            "failed to copy .worktreeinclude entries into newly created worktree {}; cleaned up the created worktree",
            prepared_worktree.path.display()
        )),
        Err(cleanup_error) => include_error.context(format!(
            "failed to copy .worktreeinclude entries into newly created worktree {}; additionally failed to clean up the created worktree: {cleanup_error:#}",
            prepared_worktree.path.display()
        )),
    }
}

fn cleanup_created_worktree_after_include_failure_inner(
    prepared_worktree: &PreparedWorktree,
    branch_cleanup: BranchCleanup,
) -> Result<()> {
    let mut cleanup_errors = Vec::new();

    if let Err(err) = run_git_cleanup_command(
        &prepared_worktree.source_root,
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            prepared_worktree.path.as_os_str(),
        ],
        "remove git worktree",
    ) {
        cleanup_errors.push(err);
    }

    match branch_cleanup {
        BranchCleanup::DeleteCreatedBranch => {
            if let Err(err) = run_git_cleanup_command(
                &prepared_worktree.source_root,
                [
                    OsStr::new("branch"),
                    OsStr::new("-D"),
                    OsStr::new(prepared_worktree.branch.as_str()),
                ],
                "delete git worktree branch",
            ) {
                cleanup_errors.push(err);
            }
        }
        BranchCleanup::PreserveExistingBranch => {}
    }

    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        let details = cleanup_errors
            .into_iter()
            .map(|err| format!("{err:#}"))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("{details}")
    }
}

fn run_git_cleanup_command<I, S>(source_root: &Path, args: I, action: &str) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(source_root)
        .output()
        .with_context(|| format!("failed to {action} in {}", source_root.display()))?;
    if !output.status.success() {
        bail!(
            "failed to {action} in {}: {}",
            source_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RemoteAppServerEndpoint;
    use codex_config::types::WorktreeBaseRef;
    use codex_git_utils::worktree::GitWorktreeBaseRef;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::process::Command;
    use uuid::Uuid;

    #[test]
    fn absent_cli_value_does_not_request_worktree() -> anyhow::Result<()> {
        assert_eq!(WorktreeRequest::from_cli_value(None)?, None);
        Ok(())
    }

    #[test]
    fn explicit_cli_name_is_used_without_rewriting() -> anyhow::Result<()> {
        let request = WorktreeRequest::from_cli_value(Some("task-4.alpha_1"))?
            .expect("worktree request should be present");

        assert_eq!(
            request,
            WorktreeRequest {
                name: "task-4.alpha_1".to_string(),
                generated_name: false,
            }
        );
        Ok(())
    }

    #[test]
    fn unsafe_explicit_cli_name_is_rejected() {
        let err = WorktreeRequest::from_cli_value(Some("../task"))
            .expect_err("path-like worktree name should be rejected");

        assert!(
            err.to_string()
                .contains("worktree name must contain only ASCII letters")
        );
    }

    #[test]
    fn empty_cli_value_generates_slug_safe_name() -> anyhow::Result<()> {
        let uuid = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000")?;

        assert_eq!(
            WorktreeRequest::generated_from_uuid(uuid),
            WorktreeRequest {
                name: "session-123e4567e89b12d3a456426614174000".to_string(),
                generated_name: true,
            }
        );
        Ok(())
    }

    #[test]
    fn maps_config_base_ref_to_git_base_ref() {
        assert_eq!(
            git_base_ref_from_config(WorktreeBaseRef::Fresh),
            GitWorktreeBaseRef::Fresh
        );
        assert_eq!(
            git_base_ref_from_config(WorktreeBaseRef::Head),
            GitWorktreeBaseRef::Head
        );
    }

    #[test]
    fn app_server_target_validation_rejects_local_daemon_before_cwd_resolution()
    -> anyhow::Result<()> {
        let target = AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: AbsolutePathBuf::relative_to_current_dir("codex.sock")?,
            },
        };

        let err = validate_app_server_target(Some("task"), &target)
            .expect_err("local daemon worktree launch should be rejected");

        assert!(err.to_string().contains("not supported"));
        Ok(())
    }

    #[test]
    fn app_server_target_validation_rejects_remote_before_cwd_resolution() -> anyhow::Result<()> {
        let target = AppServerTarget::Remote {
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: AbsolutePathBuf::relative_to_current_dir("codex.sock")?,
            },
        };

        let err = validate_app_server_target(Some("task"), &target)
            .expect_err("remote worktree launch should be rejected");

        assert!(err.to_string().contains("not supported"));
        Ok(())
    }

    #[test]
    fn repo_hint_keeps_no_worktree_daemon_launches_unchanged() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let cwd = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;
        let target = AppServerTarget::LocalDaemon {
            endpoint: RemoteAppServerEndpoint::UnixSocket {
                socket_path: AbsolutePathBuf::relative_to_current_dir("codex.sock")?,
            },
        };

        let repo_hint = repo_hint_for_target(None, &target, Some(&cwd))?;

        assert_eq!(repo_hint, None);
        Ok(())
    }

    #[test]
    fn repo_hint_allows_embedded_target() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let cwd = AbsolutePathBuf::from_absolute_path(temp_dir.path())?;

        let repo_hint = repo_hint_for_target(Some("task"), &AppServerTarget::Embedded, Some(&cwd))?;

        assert_eq!(repo_hint, Some(cwd.as_path()));
        Ok(())
    }

    #[test]
    fn final_cwd_override_uses_prepared_worktree_path() {
        let source_cwd = PathBuf::from("/repo");
        let worktree_path = PathBuf::from("/repo/.codex/worktrees/task");
        let prepared_worktree = PreparedWorktree {
            source_root: source_cwd.clone(),
            path: worktree_path.clone(),
            branch: "codex-worktree-task".to_string(),
            base_ref: "HEAD".to_string(),
            created: true,
            generated_name: false,
        };

        let cwd_override = final_cwd_override_for_launch(
            /*uses_remote_workspace*/ false,
            Some(source_cwd),
            Some(&prepared_worktree),
        );

        assert_eq!(cwd_override, Some(worktree_path));
    }

    #[test]
    fn final_cwd_override_preserves_explicit_cwd_without_worktree() {
        let explicit_cwd = PathBuf::from("/repo/subdir");

        let cwd_override = final_cwd_override_for_launch(
            /*uses_remote_workspace*/ false,
            Some(explicit_cwd.clone()),
            /*prepared_worktree*/ None,
        );

        assert_eq!(cwd_override, Some(explicit_cwd));
    }

    #[test]
    fn final_cwd_override_preserves_cwd_relative_to_source_repo_root() {
        let source_root = PathBuf::from("/repo");
        let source_subdir = source_root.join("subdir");
        let worktree_path = source_root.join(".codex").join("worktrees").join("task");
        let prepared_worktree = PreparedWorktree {
            source_root,
            path: worktree_path.clone(),
            branch: "codex-worktree-task".to_string(),
            base_ref: "HEAD".to_string(),
            created: true,
            generated_name: false,
        };

        let cwd_override = final_cwd_override_for_launch(
            /*uses_remote_workspace*/ false,
            Some(source_subdir),
            Some(&prepared_worktree),
        );

        assert_eq!(cwd_override, Some(worktree_path.join("subdir")));
    }

    #[test]
    fn validation_rejects_resume_last() {
        let err = validate_starts_new_session(
            Some("task"),
            WorktreeLaunchMode {
                resume_last: true,
                ..Default::default()
            },
        )
        .expect_err("resume --last with worktree should be rejected");

        assert_eq!(
            err,
            "--worktree is only supported when starting a new interactive session"
        );
    }

    #[test]
    fn validation_rejects_fork_last() {
        let err = validate_starts_new_session(
            Some("task"),
            WorktreeLaunchMode {
                fork_last: true,
                ..Default::default()
            },
        )
        .expect_err("fork --last with worktree should be rejected");

        assert_eq!(
            err,
            "--worktree is only supported when starting a new interactive session"
        );
    }

    #[test]
    fn validation_rejects_picker_modes() {
        for launch_mode in [
            WorktreeLaunchMode {
                resume_picker: true,
                ..Default::default()
            },
            WorktreeLaunchMode {
                fork_picker: true,
                ..Default::default()
            },
        ] {
            let err = validate_starts_new_session(Some("task"), launch_mode)
                .expect_err("picker mode with worktree should be rejected");

            assert_eq!(
                err,
                "--worktree is only supported when starting a new interactive session"
            );
        }
    }

    #[test]
    fn validation_allows_new_session_and_no_worktree_resume_or_fork() {
        assert_eq!(
            validate_starts_new_session(Some("task"), WorktreeLaunchMode::default()),
            Ok(())
        );
        assert_eq!(
            validate_starts_new_session(
                None,
                WorktreeLaunchMode {
                    resume_last: true,
                    ..Default::default()
                },
            ),
            Ok(())
        );
    }

    #[test]
    fn prepare_launch_worktree_cleans_up_created_worktree_after_include_failure()
    -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        fs::create_dir_all(&source_root)?;
        run_git(&source_root, ["init", "-q"])?;
        run_git(&source_root, ["config", "user.email", "codex@example.com"])?;
        run_git(&source_root, ["config", "user.name", "Codex"])?;
        fs::write(source_root.join(".worktreeinclude"), "../outside\n")?;
        run_git(&source_root, ["add", ".worktreeinclude"])?;
        run_git(&source_root, ["commit", "-qm", "init"])?;

        let err =
            prepare_launch_worktree(&source_root, Some("include-failure"), WorktreeBaseRef::Head)
                .expect_err("invalid include should fail launch preparation");

        assert!(format!("{err:#}").contains("unsafe .worktreeinclude pattern"));
        assert!(
            !source_root
                .join(".codex")
                .join("worktrees")
                .join("include-failure")
                .exists()
        );
        assert!(!git_branch_exists(
            &source_root,
            "codex-worktree-include-failure"
        )?);
        Ok(())
    }

    fn run_git<I, S>(cwd: &Path, args: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = Command::new("git").args(args).current_dir(cwd).output()?;
        if !output.status.success() {
            bail!(
                "git command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn git_branch_exists(cwd: &Path, branch: &str) -> anyhow::Result<bool> {
        let output = Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(cwd)
            .output()?;
        Ok(output.status.success())
    }
}
