# Codex CLI Worktree Mode Design

## Goal

Add a Codex CLI worktree mode similar to Claude Code's `-w` / `--worktree`: a user can start an interactive Codex session in an isolated Git worktree, with Codex creating the worktree at launch and asking whether to keep or remove it on exit.

## Source Behavior

Claude Code documents `claude --worktree <name>` and `claude -w <name>` as launching Claude in a linked Git worktree under `<repo>/.claude/worktrees/<name>/`. If no name is provided, Claude generates one. Current Claude docs describe branch names as `worktree-<name>`, a default base mode of `worktree.baseRef = "fresh"` that starts from the remote default branch, `worktree.baseRef = "head"` to branch from local `HEAD`, `.worktreeinclude` for copying ignored files such as `.env`, and an interactive exit cleanup flow that keeps or removes the worktree depending on user choice.

Codex should match the core startup and cleanup behavior while using Codex paths and configuration conventions.

Sources reviewed on 2026-07-27:

- https://code.claude.com/docs/en/worktrees
- https://code.claude.com/docs/en/cli-usage
- https://code.claude.com/docs/en/settings
- https://code.claude.com/docs/en/hooks

No public screenshot of Claude's exact keep/remove exit prompt was found. The prompt behavior is documented in the Claude worktrees docs.

## Selected Scope

Implement a first patch owned by `codex-cli`:

- Add `codex -w [name]` and `codex --worktree [name]` for interactive sessions.
- Resolve the source checkout from the current working directory or `--cd`.
- Create or reopen the worktree under `<source-checkout>/.codex/worktrees/<name>`.
- Use branch `worktree-<name>` for newly created worktrees.
- Start the existing TUI with the worktree path as its effective cwd.
- On interactive exit, inspect the Codex-owned worktree and prompt the user to keep or remove it.
- Keep app-server protocol unchanged by expressing the worktree as the existing cwd and workspace root.

This first patch intentionally excludes PR-number worktrees, hook replacement, sparse checkout, symlinked directories, in-session worktree switching, Desktop worktree settings, and subagent worktree isolation.

## Alternatives Considered

### CLI-Only Worktree Lifecycle

Codex creates the worktree before TUI startup and passes the worktree directory through the existing cwd override path. Cleanup happens after the TUI returns `AppExitInfo`.

This is the recommended first patch because it is narrow, avoids app-server protocol churn, and uses existing cwd/session plumbing.

### TUI/App-Server Managed Worktrees

The TUI or app-server would accept worktree creation requests and persist worktree ownership metadata with thread state.

This could support remote workspaces, Desktop, and in-session worktree operations, but it expands the API surface and makes cleanup a cross-process concern. It should wait until CLI behavior is proven.

### Git-Only Wrapper

Codex could document `git worktree add ... && codex -C ...` without adding a first-class flag.

This is too weak for the requested parity because it does not provide naming, default path, copied ignored files, or exit cleanup.

## User Interface

### Startup

The interactive CLI accepts:

```text
codex --worktree
codex --worktree feature-name
codex -w
codex -w feature-name
codex -C path/to/repo --worktree feature-name
```

`--worktree` without a name generates a readable unique name. The exact generator can be simple in the first patch as long as names are filesystem-safe and collision-resistant.

`--worktree` applies only to local interactive sessions in the first patch. If a remote app-server or remote workspace is selected, Codex rejects the option with a clear error instead of creating a local worktree that the remote session cannot use.

### Path and Branch Defaults

For source checkout `/repo/project` and name `feature-name`:

- Worktree path: `/repo/project/.codex/worktrees/feature-name`
- Branch name: `worktree-feature-name`

The source checkout is the Git repository root for the effective launch directory. If `--cd` is present, Codex resolves the source checkout from `--cd`; otherwise it resolves from the process cwd.

If the chosen path already exists and is a Git worktree, Codex reuses it. If the path exists but is not the expected worktree, startup fails with a clear message.

### Exit Cleanup

On interactive exit, Codex checks only the worktree it created or reopened for this session. It must not remove arbitrary directories based only on cwd.

Codex asks whether to keep or remove when the worktree has uncommitted changes, untracked files, or commits not present on the base branch. Keeping preserves the directory and branch. Removing runs `git worktree remove` and deletes the branch only when Codex created that branch and it is safe to delete.

For an unnamed generated worktree that is clean and has no new commits, Codex may remove it automatically. For a named worktree, Codex should prompt before removal even when clean.

If cleanup fails, Codex reports the path and leaves the worktree intact.

## Configuration

Add a `[worktree]` config group rather than placing creation settings under `[tui]`. Worktree creation affects process startup, Git state, and cleanup; it is not only TUI behavior.

Initial fields:

```toml
[worktree]
base_ref = "fresh" # "fresh" or "head"
```

`fresh` means branch from the remote default branch when available, normally `origin/HEAD` or the remote default branch resolved through Git. If the remote default cannot be resolved, fall back to local `HEAD` with a warning. `head` means branch from the current local `HEAD`.

Do not add arbitrary refs in the first patch. They can be added later if a real workflow needs them.

Because this changes `ConfigToml`, the implementation must regenerate `codex-rs/core/config.schema.json` with `just write-config-schema`.

## Ignored File Copying

Support `.worktreeinclude` in the source checkout root. The file uses `.gitignore`-style path patterns and lists ignored or untracked files that should be copied into a newly created worktree.

The first patch should copy regular files and directories that match explicit patterns and exist under the source checkout. It should reject paths that escape the checkout root. Symlinks can be deferred unless the existing filesystem helpers make safe copying straightforward.

If `.worktreeinclude` is missing, Codex copies nothing beyond Git's checkout contents.

## Implementation Boundaries

Use a new focused worktree helper module instead of growing central TUI files:

- `codex-rs/git-utils/src/worktree.rs`: typed Git worktree operations, base ref resolution, dirty/new-commit checks, add/remove, branch cleanup.
- `codex-rs/cli/src/worktree.rs`: CLI orchestration, generated names, `.worktreeinclude` copying, cleanup prompt wiring.
- `codex-rs/utils/cli/src/shared_options.rs` or `codex-rs/tui/src/cli.rs`: CLI flag definition, depending on whether `exec` support is included. For the first patch, prefer interactive-only ownership unless implementation shows that shared parsing is cleaner without enabling `exec`.
- `codex-rs/config/src/config_toml.rs`, `codex-rs/config/src/types.rs`, and `codex-rs/core/src/config/mod.rs`: config type and runtime mapping.

Avoid app-server protocol changes in the first patch. Existing thread start/resume/fork params already carry cwd and runtime workspace roots.

## Precedence

`--cd` selects the source checkout. `--worktree` then creates or reopens a worktree from that source checkout and replaces the final session cwd with the worktree path.

For `codex resume` and `codex fork`, `--worktree` should be rejected in the first patch unless the implementation can make the interaction obvious. Resuming or forking into a newly created worktree has more policy questions than starting a fresh session, especially with `tui.resume_cwd`.

## Testing

Use TDD for implementation.

Expected coverage:

- CLI parse tests for `--worktree`, `-w`, optional name, and rejection with unsupported modes.
- Git utility tests with temporary repositories for base ref resolution, worktree creation/reuse, dirty detection, untracked detection, ahead-of-base detection, removal, and safe branch deletion.
- Config tests for `[worktree] base_ref`.
- Schema regeneration check through `just write-config-schema`.
- CLI orchestration tests for `--cd` source checkout precedence and `.worktreeinclude` copying.
- Exit cleanup prompt tests using the existing CLI `confirm` style, or a small injectable prompt abstraction if needed.

Routine verification after code changes:

- `just fmt`
- `just test -p codex-git-utils`
- `just test -p codex-cli`
- `just test -p codex-core` if config runtime mapping changes
- Ask before running full `just test`.

## Risks

- Cleanup can destroy user work if Codex confuses an arbitrary cwd with a Codex-owned worktree. The implementation must track ownership from the launch operation and refuse cleanup outside that record.
- Creating a worktree under `.codex/worktrees` means repositories should ignore that path. The implementation should warn if the path is not ignored, but it should not rewrite `.gitignore` automatically in the first patch.
- Remote workspaces need server-side worktree creation. Rejecting them in the first patch is clearer than creating a local worktree the session cannot use.
- Worktree branch deletion must be conservative. If deletion is not provably safe, leave the branch and report it.
