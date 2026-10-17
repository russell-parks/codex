# Codex fork post-maintenance instructions

These instructions belong to `fork-maintenance/bootstrap` and travel with the patch stack. Keep repository-specific release policy here rather than in the shared skill.

## After maintenance

Report the rebuilt integration commit and verification results. Build or publish releases only when requested by the user; a routine maintenance run does not authorize publication. Honor publication authorization already given in the session.

## Historical release builds

- Resolve the exact upstream `openai/codex` release tag `rust-v<VERSION>` and its peeled commit SHA. Build each `release/fork-<VERSION>` from that SHA and apply the active manifest features in order, resolving compatibility issues on the owning fork branches. Do not substitute current integration as the historical base.
- Check that the upstream release contains the worktree foundation and the later model-callable TUI worktree tools needed by our extension. Report missing support before selecting historical versions. The currently selected historical releases are 0.162.1 and 0.163.0; 0.160.x and 0.161.x were excluded.
- Verify the affected crates and features, then build and smoke-test the actual packaged runtime. Include required helpers, resources, executable permissions, source/version metadata, and checksums.

## GitHub Releases

- A pushed tag or an Actions artifact is not a completed release. Publish a GitHub Release at `fork-v<VERSION>` in `russell-parks/codex`, titled `<VERSION>` to match upstream.
- Fetch the matching upstream release body and preserve it in full. Append clearly labeled fork features and fixes derived from the active branches actually included in this build. Exclude inactive features and distinguish temporary workarounds from new features.
- Publish stable versions with `draft=false` and `prerelease=false`. Only the newest stable version should become latest; historical backfills must not replace a newer latest release.
- Upload only binaries built from our fork. Report supported platforms explicitly; do not copy upstream binaries as if they contain our changes. Current published assets cover macOS arm64.
- Preserve existing release tags and asset names used by Homebrew. Never silently replace published binaries or checksums. For a correction, inspect the existing release and update its metadata or notes without recreating its assets.
- Verify the published title, body, stable/latest status, asset downloads, and checksums. Report links to the actual Releases.

## Homebrew

Maintain our recipes in `~/src/russell-parks/homebrew-tap` (`russell-parks/homebrew-tap`). The `codex-fork` formula follows our latest stable build; `codex-fork@0.162` and `codex-fork@0.163` pin their minor series. Keep them keg-only because they share the `codex` command with the upstream cask. Pin real fork release asset URLs and SHA256 values, and verify packaging and formula behavior before publishing tap updates.

## Build and publication mechanisms

Use the fork artifact workflow when available, or a verified local build when appropriate. Check the actual workflow at the selected release ref: integration artifacts alone do not publish a release, and the inherited upstream workflow expects `rust-v*` tags and upstream infrastructure. GitHub Actions previously rejected dispatches as disabled for this repository; verify its current state rather than assuming the problem persists or a tag triggers a build. Apply the release policy above regardless of the build mechanism.
