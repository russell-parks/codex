use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

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
use globset::GlobBuilder;
use globset::GlobSet;
use globset::GlobSetBuilder;
use uuid::Uuid;

const WORKTREE_INCLUDE_FILE: &str = ".worktreeinclude";

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

    let prepared_worktree = prepare_worktree(
        repo_hint,
        WorktreePrepareOptions {
            name: request.name,
            base_ref: git_base_ref_from_config(base_ref),
            generated_name: request.generated_name,
        },
    )?;
    copy_worktree_include_files(&prepared_worktree)?;
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

fn copy_worktree_include_files(prepared_worktree: &PreparedWorktree) -> Result<()> {
    if !prepared_worktree.created {
        return Ok(());
    }

    let include_path = prepared_worktree.source_root.join(WORKTREE_INCLUDE_FILE);
    let contents = match std::fs::read_to_string(&include_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", include_path.display()));
        }
    };
    let matcher = WorktreeIncludeMatcher::from_contents(&contents)?;
    if matcher.is_empty() {
        return Ok(());
    }

    copy_matching_worktree_include_entries(
        &prepared_worktree.source_root,
        &prepared_worktree.path,
        Path::new(""),
        &matcher,
    )
}

struct WorktreeIncludeMatcher {
    globset: GlobSet,
}

impl WorktreeIncludeMatcher {
    fn from_contents(contents: &str) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();

        for (line_index, line) in contents.lines().enumerate() {
            let line_number = line_index + 1;
            let pattern = line.trim();
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }

            validate_worktree_include_pattern(pattern, line_number)?;
            add_worktree_include_pattern(&mut builder, pattern, line_number)?;
        }

        Ok(Self {
            globset: builder
                .build()
                .context("failed to build .worktreeinclude matcher")?,
        })
    }

    fn is_empty(&self) -> bool {
        self.globset.is_empty()
    }

    fn is_match(&self, relative_path: &Path) -> bool {
        self.globset.is_match(relative_path)
    }
}

fn validate_worktree_include_pattern(pattern: &str, line_number: usize) -> Result<()> {
    for component in Path::new(pattern).components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                bail!("unsafe .worktreeinclude pattern on line {line_number}: {pattern:?}");
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

fn add_worktree_include_pattern(
    builder: &mut GlobSetBuilder,
    pattern: &str,
    line_number: usize,
) -> Result<()> {
    let directory_pattern = pattern.ends_with('/');
    let pattern = pattern.trim_end_matches('/');
    if pattern.is_empty() {
        bail!("unsafe .worktreeinclude pattern on line {line_number}: {pattern:?}");
    }

    add_glob(builder, pattern, line_number)?;
    if directory_pattern {
        add_glob(builder, &format!("{pattern}/**"), line_number)?;
    }

    Ok(())
}

fn add_glob(builder: &mut GlobSetBuilder, pattern: &str, line_number: usize) -> Result<()> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .with_context(|| {
            format!("invalid .worktreeinclude pattern on line {line_number}: {pattern:?}")
        })?;
    builder.add(glob);
    Ok(())
}

fn copy_matching_worktree_include_entries(
    source_root: &Path,
    target_root: &Path,
    relative_dir: &Path,
    matcher: &WorktreeIncludeMatcher,
) -> Result<()> {
    let source_dir = source_root.join(relative_dir);
    for entry in std::fs::read_dir(&source_dir)
        .with_context(|| format!("failed to read directory {}", source_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!("failed to read directory entry in {}", source_dir.display())
        })?;
        let relative_path = relative_dir.join(entry.file_name());
        if is_forbidden_worktree_include_path(&relative_path) {
            continue;
        }

        let source_path = source_root.join(&relative_path);
        let metadata = std::fs::symlink_metadata(&source_path)
            .with_context(|| format!("failed to inspect {}", source_path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            // Avoid following or recreating links whose target may escape either checkout.
            continue;
        }

        if file_type.is_dir() {
            if matcher.is_match(&relative_path) {
                create_worktree_include_target_dir(target_root, &relative_path)?;
            }
            copy_matching_worktree_include_entries(
                source_root,
                target_root,
                &relative_path,
                matcher,
            )?;
        } else if file_type.is_file() && matcher.is_match(&relative_path) {
            copy_worktree_include_file(source_root, target_root, &relative_path)?;
        }
    }
    Ok(())
}

fn create_worktree_include_target_dir(target_root: &Path, relative_path: &Path) -> Result<()> {
    let target_path = safe_worktree_include_target_path(target_root, relative_path)?;
    match std::fs::symlink_metadata(&target_path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => bail!(
            "cannot copy directory {} because target path already exists as a file",
            target_path.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&target_path)
                .with_context(|| format!("failed to create directory {}", target_path.display()))
        }
        Err(err) => {
            Err(err).with_context(|| format!("failed to inspect {}", target_path.display()))
        }
    }
}

fn copy_worktree_include_file(
    source_root: &Path,
    target_root: &Path,
    relative_path: &Path,
) -> Result<()> {
    let source_path = source_root.join(relative_path);
    let target_path = safe_worktree_include_target_path(target_root, relative_path)?;
    match std::fs::symlink_metadata(&target_path) {
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to inspect {}", target_path.display()));
        }
    }

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    std::fs::copy(&source_path, &target_path).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source_path.display(),
            target_path.display()
        )
    })?;
    Ok(())
}

fn safe_worktree_include_target_path(target_root: &Path, relative_path: &Path) -> Result<PathBuf> {
    for component in relative_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                bail!(
                    "unsafe .worktreeinclude target path under {}: {}",
                    target_root.display(),
                    relative_path.display()
                );
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    let target_path = target_root.join(relative_path);
    if !target_path.starts_with(target_root) {
        bail!(
            "unsafe .worktreeinclude target path under {}: {}",
            target_root.display(),
            relative_path.display()
        );
    }
    Ok(target_path)
}

fn is_forbidden_worktree_include_path(relative_path: &Path) -> bool {
    let mut components = relative_path.components();
    match components.next() {
        Some(Component::Normal(name)) if name == ".git" => true,
        Some(Component::Normal(name)) if name == ".codex" => {
            matches!(components.next(), Some(Component::Normal(child)) if child == "worktrees")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RemoteAppServerEndpoint;
    use codex_config::types::WorktreeBaseRef;
    use codex_git_utils::worktree::GitWorktreeBaseRef;
    use pretty_assertions::assert_eq;
    use std::fs;
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
    fn worktreeinclude_missing_file_is_no_op() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(&source_root)?;
        fs::create_dir_all(&target_root)?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))?;

        assert!(!target_root.join(".env").exists());
        Ok(())
    }

    #[test]
    fn worktreeinclude_copies_explicit_and_globbed_files_preserving_paths() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(source_root.join("config"))?;
        fs::create_dir_all(&target_root)?;
        fs::write(
            source_root.join(".worktreeinclude"),
            ".env\n.env.*\nconfig/*.local\n",
        )?;
        fs::write(source_root.join(".env"), "base")?;
        fs::write(source_root.join(".env.local"), "local")?;
        fs::write(source_root.join("config").join("dev.local"), "dev")?;
        fs::write(source_root.join("config").join("dev.toml"), "tracked")?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))?;

        assert_eq!(fs::read_to_string(target_root.join(".env"))?, "base");
        assert_eq!(fs::read_to_string(target_root.join(".env.local"))?, "local");
        assert_eq!(
            fs::read_to_string(target_root.join("config").join("dev.local"))?,
            "dev"
        );
        assert!(!target_root.join("config").join("dev.toml").exists());
        Ok(())
    }

    #[test]
    fn worktreeinclude_copies_directory_style_pattern_recursively() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(source_root.join(".codex").join("skills").join("rust"))?;
        fs::create_dir_all(&target_root)?;
        fs::write(source_root.join(".worktreeinclude"), ".codex/skills/\n")?;
        fs::write(
            source_root
                .join(".codex")
                .join("skills")
                .join("rust")
                .join("SKILL.md"),
            "rust skill",
        )?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))?;

        assert_eq!(
            fs::read_to_string(
                target_root
                    .join(".codex")
                    .join("skills")
                    .join("rust")
                    .join("SKILL.md")
            )?,
            "rust skill"
        );
        Ok(())
    }

    #[test]
    fn worktreeinclude_ignores_comments_and_blank_lines() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(&source_root)?;
        fs::create_dir_all(&target_root)?;
        fs::write(
            source_root.join(".worktreeinclude"),
            "\n# local env\n\n.env\n",
        )?;
        fs::write(source_root.join(".env"), "secret")?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))?;

        assert_eq!(fs::read_to_string(target_root.join(".env"))?, "secret");
        Ok(())
    }

    #[test]
    fn worktreeinclude_does_not_copy_git_or_generated_worktrees() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(source_root.join(".git").join("objects"))?;
        fs::create_dir_all(
            source_root
                .join(".codex")
                .join("worktrees")
                .join("other")
                .join(".codex"),
        )?;
        fs::create_dir_all(&target_root)?;
        fs::write(
            source_root.join(".worktreeinclude"),
            ".git/\n.codex/worktrees/\n",
        )?;
        fs::write(source_root.join(".git").join("config"), "git config")?;
        fs::write(
            source_root
                .join(".codex")
                .join("worktrees")
                .join("other")
                .join(".codex")
                .join("state.json"),
            "state",
        )?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))?;

        assert!(!target_root.join(".git").exists());
        assert!(!target_root.join(".codex").join("worktrees").exists());
        Ok(())
    }

    #[test]
    fn worktreeinclude_reused_worktree_is_not_mutated() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(&source_root)?;
        fs::create_dir_all(&target_root)?;
        fs::write(source_root.join(".worktreeinclude"), ".env\n")?;
        fs::write(source_root.join(".env"), "secret")?;

        copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ false,
        ))?;

        assert!(!target_root.join(".env").exists());
        Ok(())
    }

    #[test]
    fn worktreeinclude_rejects_unsafe_traversal_patterns() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let source_root = temp_dir.path().join("source");
        let target_root = temp_dir.path().join("target");
        fs::create_dir_all(&source_root)?;
        fs::create_dir_all(&target_root)?;
        fs::write(source_root.join(".worktreeinclude"), "../outside\n")?;

        let err = copy_worktree_include_files(&prepared_worktree(
            &source_root,
            &target_root,
            /*created*/ true,
        ))
        .expect_err("traversal pattern should be rejected");

        assert!(err.to_string().contains("unsafe .worktreeinclude pattern"));
        Ok(())
    }

    fn prepared_worktree(
        source_root: &Path,
        target_root: &Path,
        created: bool,
    ) -> PreparedWorktree {
        PreparedWorktree {
            source_root: source_root.to_path_buf(),
            path: target_root.to_path_buf(),
            branch: "codex-worktree-task".to_string(),
            base_ref: "HEAD".to_string(),
            created,
            generated_name: false,
        }
    }
}
