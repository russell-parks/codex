use std::path::Path;

use anyhow::Result;
use anyhow::bail;
use codex_config::types::WorktreeBaseRef;
use codex_git_utils::worktree::GitWorktreeBaseRef;
use codex_git_utils::worktree::PreparedWorktree;
use codex_git_utils::worktree::WorktreePrepareOptions;
use codex_git_utils::worktree::prepare_worktree;
use uuid::Uuid;

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

    prepare_worktree(
        repo_hint,
        WorktreePrepareOptions {
            name: request.name,
            base_ref: git_base_ref_from_config(base_ref),
            generated_name: request.generated_name,
        },
    )
    .map(Some)
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

#[cfg(test)]
mod tests {
    use super::*;
    use codex_config::types::WorktreeBaseRef;
    use codex_git_utils::worktree::GitWorktreeBaseRef;
    use pretty_assertions::assert_eq;
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
}
