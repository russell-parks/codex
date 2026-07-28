use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::bail;
use codex_git_utils::worktree::PreparedWorktree;
use pretty_assertions::assert_eq;

use super::WorktreeIncludeMatcher;
use super::WorktreeIncludeSourceFilter;
use super::copy_worktree_include_files;
use super::copy_worktree_include_files_with_source_filter;

#[test]
fn worktreeinclude_missing_file_is_no_op() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;

    copy_worktree_include_files_allow_all(&prepared_worktree(
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

    copy_worktree_include_files_allow_all(&prepared_worktree(
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

#[cfg(unix)]
#[test]
fn worktreeinclude_preserves_copied_file_mode() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;
    fs::write(source_root.join(".worktreeinclude"), ".env\n")?;
    fs::write(source_root.join(".env"), "secret")?;
    fs::set_permissions(source_root.join(".env"), fs::Permissions::from_mode(0o600))?;

    copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert_eq!(
        fs::metadata(target_root.join(".env"))?.permissions().mode() & 0o777,
        0o600
    );
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

    copy_worktree_include_files_allow_all(&prepared_worktree(
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

    copy_worktree_include_files_allow_all(&prepared_worktree(
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

    copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert!(!target_root.join(".git").exists());
    assert!(!target_root.join(".codex").join("worktrees").exists());
    Ok(())
}

#[test]
fn worktreeinclude_does_not_copy_nested_git_directories() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(source_root.join("vendor").join(".git").join("objects"))?;
    fs::create_dir_all(&target_root)?;
    fs::write(source_root.join(".worktreeinclude"), "vendor/\n")?;
    fs::write(
        source_root.join("vendor").join(".git").join("config"),
        "git",
    )?;
    fs::write(source_root.join("vendor").join("README.md"), "vendored")?;

    copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert_eq!(
        fs::read_to_string(target_root.join("vendor").join("README.md"))?,
        "vendored"
    );
    assert!(!target_root.join("vendor").join(".git").exists());
    Ok(())
}

#[test]
fn worktreeinclude_derives_literal_walk_roots() -> anyhow::Result<()> {
    let matcher = WorktreeIncludeMatcher::from_contents(".env\nconfig/*.local\n.codex/skills/\n")?;

    assert_eq!(
        matcher.walk_roots,
        vec![
            PathBuf::from(".env"),
            PathBuf::from("config"),
            PathBuf::from(".codex").join("skills"),
        ]
    );
    Ok(())
}

#[test]
fn worktreeinclude_root_level_glob_does_not_match_descendant_directories() -> anyhow::Result<()> {
    let matcher = WorktreeIncludeMatcher::from_contents(".env.*\n")?;

    assert!(!matcher.may_match_descendant(Path::new("nested")));

    let mixed_matcher = WorktreeIncludeMatcher::from_contents(".env.*\nconfig/*.local\n")?;
    assert!(mixed_matcher.may_match_descendant(Path::new("config")));
    assert!(!mixed_matcher.may_match_descendant(Path::new("nested")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn worktreeinclude_root_level_glob_does_not_traverse_nested_directories() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    let nested_dir = source_root.join("nested");
    fs::create_dir_all(&nested_dir)?;
    fs::create_dir_all(&target_root)?;
    fs::write(source_root.join(".worktreeinclude"), ".env.*\n")?;
    fs::write(source_root.join(".env.local"), "root")?;
    fs::write(nested_dir.join(".env.local"), "nested")?;
    fs::set_permissions(&nested_dir, fs::Permissions::from_mode(0o000))?;

    let result = copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ));
    fs::set_permissions(&nested_dir, fs::Permissions::from_mode(0o700))?;
    result?;

    assert_eq!(fs::read_to_string(target_root.join(".env.local"))?, "root");
    assert!(!target_root.join("nested").exists());
    Ok(())
}

#[test]
fn worktreeinclude_derives_git_status_pathspecs_from_walk_roots() -> anyhow::Result<()> {
    let matcher =
        WorktreeIncludeMatcher::from_contents(".env\nconfig/*.local\nparent/\nnested/**/file\n")?;

    assert_eq!(
        matcher.git_status_pathspecs(),
        vec![
            PathBuf::from(".env"),
            PathBuf::from("config"),
            PathBuf::from("parent"),
            PathBuf::from("nested"),
        ]
    );
    let root_wide_matcher = WorktreeIncludeMatcher::from_contents("*/root-wide\n")?;
    assert_eq!(
        root_wide_matcher.git_status_pathspecs(),
        vec![PathBuf::from(".")]
    );
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

    let err = copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))
    .expect_err("traversal pattern should be rejected");

    assert!(format!("{err:#}").contains("unsafe .worktreeinclude pattern"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn worktreeinclude_rejects_existing_target_symlink_ancestors() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    let outside_root = temp_dir.path().join("outside");
    fs::create_dir_all(source_root.join(".codex").join("skills").join("rust"))?;
    fs::create_dir_all(&target_root)?;
    fs::create_dir_all(&outside_root)?;
    std::os::unix::fs::symlink(&outside_root, target_root.join(".codex"))?;
    fs::write(source_root.join(".worktreeinclude"), ".codex/skills/\n")?;
    fs::write(
        source_root
            .join(".codex")
            .join("skills")
            .join("rust")
            .join("SKILL.md"),
        "rust skill",
    )?;

    let err = copy_worktree_include_files_allow_all(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))
    .expect_err("target symlink ancestor should be rejected");

    assert!(err.to_string().contains("refusing to copy"));
    assert!(!outside_root.join("skills").exists());
    Ok(())
}

#[test]
fn worktreeinclude_copies_only_untracked_or_ignored_source_files() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;
    run_git(&source_root, ["init", "-q"])?;
    run_git(&source_root, ["config", "user.email", "codex@example.com"])?;
    run_git(&source_root, ["config", "user.name", "Codex"])?;
    fs::write(source_root.join(".gitignore"), "ignored.secret\n")?;
    fs::write(
        source_root.join(".worktreeinclude"),
        "*.txt\nignored.secret\n",
    )?;
    fs::write(source_root.join("tracked.txt"), "tracked")?;
    fs::write(source_root.join("untracked.txt"), "untracked")?;
    fs::write(source_root.join("ignored.secret"), "ignored")?;
    run_git(&source_root, ["add", ".gitignore", "tracked.txt"])?;
    run_git(&source_root, ["commit", "-qm", "init"])?;

    copy_worktree_include_files(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert!(!target_root.join("tracked.txt").exists());
    assert_eq!(
        fs::read_to_string(target_root.join("untracked.txt"))?,
        "untracked"
    );
    assert_eq!(
        fs::read_to_string(target_root.join("ignored.secret"))?,
        "ignored"
    );
    Ok(())
}

#[test]
fn worktreeinclude_copies_empty_descendant_directories_under_ignored_directory()
-> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;
    run_git(&source_root, ["init", "-q"])?;
    run_git(&source_root, ["config", "user.email", "codex@example.com"])?;
    run_git(&source_root, ["config", "user.name", "Codex"])?;
    fs::write(source_root.join(".gitignore"), "parent/\n")?;
    fs::write(source_root.join(".worktreeinclude"), "parent/\n")?;
    fs::create_dir_all(source_root.join("parent").join("nested-empty"))?;
    run_git(&source_root, ["add", ".gitignore"])?;
    run_git(&source_root, ["commit", "-qm", "init"])?;

    copy_worktree_include_files(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert!(target_root.join("parent").join("nested-empty").is_dir());
    Ok(())
}

#[test]
fn worktreeinclude_copies_empty_descendant_directories_under_untracked_directory()
-> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;
    run_git(&source_root, ["init", "-q"])?;
    run_git(&source_root, ["config", "user.email", "codex@example.com"])?;
    run_git(&source_root, ["config", "user.name", "Codex"])?;
    fs::write(source_root.join(".worktreeinclude"), "parent/\n")?;
    fs::create_dir_all(source_root.join("parent"))?;
    fs::write(source_root.join("parent").join("tracked.txt"), "tracked")?;
    run_git(
        &source_root,
        ["add", ".worktreeinclude", "parent/tracked.txt"],
    )?;
    run_git(&source_root, ["commit", "-qm", "init"])?;
    fs::create_dir_all(source_root.join("parent").join("with-file"))?;
    fs::create_dir_all(source_root.join("parent").join("nested-empty"))?;
    fs::write(
        source_root
            .join("parent")
            .join("with-file")
            .join("file.txt"),
        "untracked",
    )?;

    copy_worktree_include_files(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert_eq!(
        fs::read_to_string(
            target_root
                .join("parent")
                .join("with-file")
                .join("file.txt")
        )?,
        "untracked"
    );
    assert!(target_root.join("parent").join("nested-empty").is_dir());
    assert!(!target_root.join("parent").join("tracked.txt").exists());
    Ok(())
}

#[test]
fn worktreeinclude_copies_empty_only_untracked_directory_root() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let source_root = temp_dir.path().join("source");
    let target_root = temp_dir.path().join("target");
    fs::create_dir_all(&source_root)?;
    fs::create_dir_all(&target_root)?;
    run_git(&source_root, ["init", "-q"])?;
    run_git(&source_root, ["config", "user.email", "codex@example.com"])?;
    run_git(&source_root, ["config", "user.name", "Codex"])?;
    fs::write(source_root.join(".worktreeinclude"), "empty-root/\n")?;
    run_git(&source_root, ["add", ".worktreeinclude"])?;
    run_git(&source_root, ["commit", "-qm", "init"])?;
    fs::create_dir_all(source_root.join("empty-root").join("nested-empty"))?;

    copy_worktree_include_files(&prepared_worktree(
        &source_root,
        &target_root,
        /*created*/ true,
    ))?;

    assert!(target_root.join("empty-root").is_dir());
    assert!(target_root.join("empty-root").join("nested-empty").is_dir());
    Ok(())
}

fn prepared_worktree(source_root: &Path, target_root: &Path, created: bool) -> PreparedWorktree {
    PreparedWorktree {
        source_root: source_root.to_path_buf(),
        path: target_root.to_path_buf(),
        branch: "codex-worktree-task".to_string(),
        base_ref: "HEAD".to_string(),
        created,
        generated_name: false,
    }
}

fn copy_worktree_include_files_allow_all(
    prepared_worktree: &PreparedWorktree,
) -> anyhow::Result<()> {
    copy_worktree_include_files_with_source_filter(
        prepared_worktree,
        &WorktreeIncludeSourceFilter::AllowAll,
    )
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
