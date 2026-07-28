use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_git_utils::worktree::PreparedWorktree;
use globset::GlobBuilder;
use globset::GlobSet;
use globset::GlobSetBuilder;

const WORKTREE_INCLUDE_FILE: &str = ".worktreeinclude";

pub(crate) fn copy_worktree_include_files(prepared_worktree: &PreparedWorktree) -> Result<()> {
    if !prepared_worktree.created {
        return Ok(());
    }

    let Some(matcher) = worktree_include_matcher(prepared_worktree)? else {
        return Ok(());
    };
    let source_filter =
        WorktreeIncludeSourceFilter::from_git_status(&prepared_worktree.source_root, &matcher)?;
    copy_matching_worktree_include_entries(
        &prepared_worktree.source_root,
        &prepared_worktree.path,
        &matcher,
        &source_filter,
    )
}

#[cfg(test)]
fn copy_worktree_include_files_with_source_filter(
    prepared_worktree: &PreparedWorktree,
    source_filter: &WorktreeIncludeSourceFilter,
) -> Result<()> {
    if !prepared_worktree.created {
        return Ok(());
    }

    let Some(matcher) = worktree_include_matcher(prepared_worktree)? else {
        return Ok(());
    };
    copy_matching_worktree_include_entries(
        &prepared_worktree.source_root,
        &prepared_worktree.path,
        &matcher,
        source_filter,
    )
}

fn worktree_include_matcher(
    prepared_worktree: &PreparedWorktree,
) -> Result<Option<WorktreeIncludeMatcher>> {
    let include_path = prepared_worktree.source_root.join(WORKTREE_INCLUDE_FILE);
    let contents = match std::fs::read_to_string(&include_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", include_path.display()));
        }
    };
    let matcher = WorktreeIncludeMatcher::from_contents(&contents)?;
    if matcher.is_empty() {
        return Ok(None);
    }

    Ok(Some(matcher))
}

struct WorktreeIncludeMatcher {
    globset: GlobSet,
    walk_roots: Vec<PathBuf>,
    directory_roots: Vec<PathBuf>,
    descend_from_root: bool,
}

enum WorktreeIncludeSourceFilter {
    #[cfg(test)]
    AllowAll,
    GitStatus {
        files: HashSet<PathBuf>,
        directory_prefixes: Vec<PathBuf>,
        untracked_directory_roots: Vec<PathBuf>,
    },
}

impl WorktreeIncludeSourceFilter {
    fn from_git_status(source_root: &Path, matcher: &WorktreeIncludeMatcher) -> Result<Self> {
        let mut command = Command::new("git");
        command
            .args([
                "status",
                "--porcelain=v1",
                "-z",
                "--ignored=matching",
                "--untracked-files=all",
                "--",
            ])
            .args(matcher.git_status_pathspecs())
            .current_dir(source_root);
        let output = command
            .output()
            .with_context(|| format!("failed to run git status in {}", source_root.display()))?;
        if !output.status.success() {
            bail!(
                "failed to inspect ignored and untracked files in {}: {}",
                source_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let mut files = HashSet::new();
        let mut directory_prefixes = Vec::new();
        for entry in output.stdout.split(|byte| *byte == 0) {
            if !entry.starts_with(b"?? ") && !entry.starts_with(b"!! ") {
                continue;
            }

            let mut path = &entry[3..];
            let directory = path.ends_with(b"/");
            if directory {
                while path.ends_with(b"/") {
                    path = &path[..path.len() - 1];
                }
            }
            if path.is_empty() {
                continue;
            }

            let relative_path = path_buf_from_git_status_path(path)?;
            validate_worktree_include_relative_path(&relative_path)?;
            if directory {
                directory_prefixes.push(relative_path);
            } else {
                files.insert(relative_path);
            }
        }
        let untracked_directory_roots = untracked_directory_roots(source_root, matcher)?;

        Ok(Self::GitStatus {
            files,
            directory_prefixes,
            untracked_directory_roots,
        })
    }

    fn allows_file(&self, relative_path: &Path) -> bool {
        match self {
            #[cfg(test)]
            Self::AllowAll => true,
            Self::GitStatus {
                files,
                directory_prefixes,
                untracked_directory_roots: _,
            } => {
                files.contains(relative_path)
                    || directory_prefixes
                        .iter()
                        .any(|prefix| relative_path.starts_with(prefix))
            }
        }
    }

    fn allows_directory(&self, relative_path: &Path) -> bool {
        match self {
            #[cfg(test)]
            Self::AllowAll => true,
            Self::GitStatus {
                files,
                directory_prefixes,
                untracked_directory_roots,
            } => {
                files.iter().any(|file| file.starts_with(relative_path))
                    || directory_prefixes.iter().any(|prefix| {
                        prefix.starts_with(relative_path) || relative_path.starts_with(prefix)
                    })
                    || untracked_directory_roots.iter().any(|root| {
                        root.starts_with(relative_path) || relative_path.starts_with(root)
                    })
            }
        }
    }

    fn allows_included_directory(
        &self,
        relative_path: &Path,
        matcher: &WorktreeIncludeMatcher,
    ) -> bool {
        self.allows_directory(relative_path)
            || matcher
                .directory_roots
                .iter()
                .any(|root| relative_path.starts_with(root) && self.allows_directory(root))
    }
}

impl WorktreeIncludeMatcher {
    fn from_contents(contents: &str) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        let mut walk_roots = Vec::new();
        let mut directory_roots = Vec::new();
        let mut descend_from_root = false;

        for (line_index, line) in contents.lines().enumerate() {
            let line_number = line_index + 1;
            let pattern = line.trim();
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }

            validate_worktree_include_pattern(pattern, line_number)?;
            add_worktree_include_pattern(&mut builder, pattern, line_number)?;
            add_worktree_include_walk_root(&mut walk_roots, pattern);
            add_worktree_include_directory_root(&mut directory_roots, pattern);
            if worktree_include_walk_root(pattern).as_os_str().is_empty()
                && pattern.trim_end_matches('/').contains('/')
            {
                descend_from_root = true;
            }
        }

        Ok(Self {
            globset: builder
                .build()
                .context("failed to build .worktreeinclude matcher")?,
            walk_roots,
            directory_roots,
            descend_from_root,
        })
    }

    fn is_empty(&self) -> bool {
        self.globset.is_empty()
    }

    fn is_match(&self, relative_path: &Path) -> bool {
        self.globset.is_match(relative_path)
    }

    fn may_match_descendant(&self, relative_path: &Path) -> bool {
        relative_path.as_os_str().is_empty()
            || self.walk_roots.iter().any(|root| {
                if root.as_os_str().is_empty() {
                    self.descend_from_root
                } else {
                    root.starts_with(relative_path) || relative_path.starts_with(root)
                }
            })
    }

    fn git_status_pathspecs(&self) -> Vec<PathBuf> {
        if self
            .walk_roots
            .iter()
            .any(|root| root.as_os_str().is_empty())
        {
            vec![PathBuf::from(".")]
        } else {
            self.walk_roots.clone()
        }
    }
}

fn validate_worktree_include_pattern(pattern: &str, line_number: usize) -> Result<()> {
    validate_worktree_include_relative_path(Path::new(pattern)).with_context(|| {
        format!("unsafe .worktreeinclude pattern on line {line_number}: {pattern:?}")
    })
}

fn validate_worktree_include_relative_path(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                bail!(
                    "path must stay within the worktree root: {}",
                    path.display()
                );
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

fn add_worktree_include_walk_root(roots: &mut Vec<PathBuf>, pattern: &str) {
    let root = worktree_include_walk_root(pattern);
    add_worktree_include_root(roots, root);
}

fn add_worktree_include_directory_root(roots: &mut Vec<PathBuf>, pattern: &str) {
    if !pattern.ends_with('/') {
        return;
    }

    let root = PathBuf::from(pattern.trim_end_matches('/'));
    if root.as_os_str().is_empty() || path_contains_glob_meta(&root) {
        return;
    }

    add_worktree_include_root(roots, root);
}

fn add_worktree_include_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if roots.iter().any(|existing| existing == &root) {
        return;
    }

    if root.as_os_str().is_empty() {
        roots.push(root);
        return;
    }

    roots.retain(|existing| !existing.starts_with(&root));
    if !roots
        .iter()
        .any(|existing| !existing.as_os_str().is_empty() && root.starts_with(existing))
    {
        roots.push(root);
    }
}

fn worktree_include_walk_root(pattern: &str) -> PathBuf {
    let pattern = pattern.trim_end_matches('/');
    let mut root = PathBuf::new();
    for component in Path::new(pattern).components() {
        let Component::Normal(name) = component else {
            continue;
        };
        if os_str_contains_glob_meta(name) {
            break;
        }

        root.push(name);
    }
    root
}

fn os_str_contains_glob_meta(value: &OsStr) -> bool {
    value
        .to_string_lossy()
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b'{' | b'}'))
}

fn path_contains_glob_meta(path: &Path) -> bool {
    path.components().any(
        |component| matches!(component, Component::Normal(name) if os_str_contains_glob_meta(name)),
    )
}

fn untracked_directory_roots(
    source_root: &Path,
    matcher: &WorktreeIncludeMatcher,
) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for root in &matcher.directory_roots {
        let source_path = source_root.join(root);
        let metadata = match std::fs::symlink_metadata(&source_path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to inspect {}", source_path.display()));
            }
        };
        if !metadata.file_type().is_dir() || git_has_tracked_entries_under(source_root, root)? {
            continue;
        }

        roots.push(root.clone());
    }
    Ok(roots)
}

fn git_has_tracked_entries_under(source_root: &Path, relative_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["ls-files", "-z", "--"])
        .arg(relative_path)
        .current_dir(source_root)
        .output()
        .with_context(|| {
            format!(
                "failed to inspect tracked files under {}",
                source_root.join(relative_path).display()
            )
        })?;
    if !output.status.success() {
        bail!(
            "failed to inspect tracked files under {}: {}",
            source_root.join(relative_path).display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(!output.stdout.is_empty())
}

#[cfg(unix)]
fn path_buf_from_git_status_path(path: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(std::ffi::OsString::from_vec(path.to_vec())))
}

#[cfg(not(unix))]
fn path_buf_from_git_status_path(path: &[u8]) -> Result<PathBuf> {
    let path = std::str::from_utf8(path).context("git status output path was not UTF-8")?;
    Ok(PathBuf::from(path))
}

fn copy_matching_worktree_include_entries(
    source_root: &Path,
    target_root: &Path,
    matcher: &WorktreeIncludeMatcher,
    source_filter: &WorktreeIncludeSourceFilter,
) -> Result<()> {
    for relative_root in &matcher.walk_roots {
        if relative_root.as_os_str().is_empty() {
            copy_matching_worktree_include_directory_entries(
                source_root,
                target_root,
                Path::new(""),
                matcher,
                source_filter,
            )?;
        } else {
            copy_matching_worktree_include_entry(
                source_root,
                target_root,
                relative_root,
                matcher,
                source_filter,
            )?;
        }
    }

    Ok(())
}

fn copy_matching_worktree_include_directory_entries(
    source_root: &Path,
    target_root: &Path,
    relative_dir: &Path,
    matcher: &WorktreeIncludeMatcher,
    source_filter: &WorktreeIncludeSourceFilter,
) -> Result<()> {
    let source_dir = source_root.join(relative_dir);
    for entry in std::fs::read_dir(&source_dir)
        .with_context(|| format!("failed to read directory {}", source_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!("failed to read directory entry in {}", source_dir.display())
        })?;
        let relative_path = relative_dir.join(entry.file_name());
        copy_matching_worktree_include_entry(
            source_root,
            target_root,
            &relative_path,
            matcher,
            source_filter,
        )?;
    }
    Ok(())
}

fn copy_matching_worktree_include_entry(
    source_root: &Path,
    target_root: &Path,
    relative_path: &Path,
    matcher: &WorktreeIncludeMatcher,
    source_filter: &WorktreeIncludeSourceFilter,
) -> Result<()> {
    if is_forbidden_worktree_include_path(relative_path) {
        return Ok(());
    }

    let source_path = source_root.join(relative_path);
    let metadata = match std::fs::symlink_metadata(&source_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to inspect {}", source_path.display()));
        }
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        // Avoid following or recreating links whose target may escape either checkout.
        return Ok(());
    }

    if file_type.is_dir() {
        let directory_allowed = source_filter.allows_included_directory(relative_path, matcher);
        if matcher.is_match(relative_path) && directory_allowed {
            create_worktree_include_target_dir(target_root, relative_path)?;
        }
        if directory_allowed && matcher.may_match_descendant(relative_path) {
            copy_matching_worktree_include_directory_entries(
                source_root,
                target_root,
                relative_path,
                matcher,
                source_filter,
            )?;
        }
    } else if file_type.is_file()
        && matcher.is_match(relative_path)
        && source_filter.allows_file(relative_path)
    {
        copy_worktree_include_file(source_root, target_root, relative_path)?;
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
    let source_permissions = std::fs::metadata(&source_path)
        .with_context(|| format!("failed to inspect {}", source_path.display()))?
        .permissions();
    if let Some(parent) = relative_path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_worktree_include_target_dir(target_root, parent)?;
    }

    let mut source = File::open(&source_path)
        .with_context(|| format!("failed to open {}", source_path.display()))?;
    let mut target_options = OpenOptions::new();
    target_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;

        target_options.mode(source_permissions.mode());
    }

    let mut target = match target_options.open(&target_path) {
        Ok(target) => target,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            safe_worktree_include_target_path(target_root, relative_path)?;
            return Ok(());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to create {}", target_path.display()));
        }
    };
    io::copy(&mut source, &mut target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source_path.display(),
            target_path.display()
        )
    })?;
    std::fs::set_permissions(&target_path, source_permissions)
        .with_context(|| format!("failed to set permissions on {}", target_path.display()))?;
    Ok(())
}

fn safe_worktree_include_target_path(target_root: &Path, relative_path: &Path) -> Result<PathBuf> {
    validate_worktree_include_relative_path(relative_path).with_context(|| {
        format!(
            "unsafe .worktreeinclude target path under {}: {}",
            target_root.display(),
            relative_path.display()
        )
    })?;

    let mut current_path = target_root.to_path_buf();
    for component in relative_path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                current_path.push(name);
                match std::fs::symlink_metadata(&current_path) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        bail!(
                            "refusing to copy .worktreeinclude entry through symlink target path {}",
                            current_path.display()
                        );
                    }
                    Ok(_) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => break,
                    Err(err) => {
                        return Err(err).with_context(|| {
                            format!("failed to inspect {}", current_path.display())
                        });
                    }
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                bail!(
                    "unsafe .worktreeinclude target path under {}: {}",
                    target_root.display(),
                    relative_path.display()
                );
            }
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
    let mut previous_component_was_codex = false;
    for component in relative_path.components() {
        let Component::Normal(name) = component else {
            previous_component_was_codex = false;
            continue;
        };

        if name == ".git" || previous_component_was_codex && name == "worktrees" {
            return true;
        }
        previous_component_was_codex = name == ".codex";
    }
    false
}

#[cfg(test)]
#[path = "worktree_include_tests.rs"]
mod tests;
