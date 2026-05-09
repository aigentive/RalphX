# Release Notes

This directory holds curated release notes plus managed GitHub release metadata that the release workflow will use automatically when a matching file exists.

Naming convention:

- `release-notes/v0.2.0.md`
- `release-notes/v0.2.1.md`
- `release-notes/v0.100.42.md`

Typical flow:

1. For local release prep, prefer the guided wrapper:
   - `./scripts/release.sh`
2. Review the proposal when prompted, accept it to continue, then review and edit the generated `release-notes/vX.Y.Z.md`
3. Commit it before tagging if you want the workflow-created draft release to use it automatically

Daily scheduled releases:

- `Daily Release` runs from `main`, skips when there are no commits after the latest reachable `vX.Y.Z` tag, and commits the generated `release-notes/vX.Y.Z.md` before tagging.
- Daily notes keep the curated RalphX summary first, then append a managed GitHub metadata block with pull-request attribution, first-time contributors when present, and the full changelog link.
- The scheduled workflow uses Codex CLI for both the version proposal and release-note generation, so the repository needs a `CODEX_API_KEY` secret.
- Protected-main setups may also need `RELEASE_AUTOMATION_TOKEN` with `contents:write` and `actions:write` so the workflow can push the release-prep commit/tag and dispatch `Release Build`.
- Manual `Daily Release` dispatch supports `dry_run=true` to verify generation without committing, tagging, pushing, or dispatching `Release Build`.
- Manual `Daily Release` dispatch supports `release_bump` to force `patch`, `minor`, or `major`; selecting `major` is explicit release-owner approval.
- Manual `Daily Release` dispatch also supports `release_version` to force an exact version; do not combine it with `release_bump`.
- Manual `Daily Release` dispatch supports `linux_runner`, `arm_runner`, `intel_runner`, and `macos_runner_size` to choose the Release Build runner path; Blacksmith is the default for Linux prep/metadata/publish and both macOS builds, while `macos_runner_size=larger` intentionally uses paid GitHub-hosted macOS larger runners for GitHub-hosted macOS builds.
- `arm_runner` and `intel_runner` each support `blacksmith`, `depot`, `github-hosted`, and `self-hosted`. Blacksmith and Depot Intel builds use Apple Silicon runners with Tauri's `x86_64-apple-darwin` target.
- Maintenance-only commits can avoid scheduled release prep when every commit after the latest tag includes `[skip daily-release]`, `[skip release]`, `[no daily-release]`, or `[no release]`.

Notes:

- Release proposals default to `.artifacts/release-notes/proposal-from-v<current-version>.md`
- Accepted release versions are stored in `.artifacts/release-notes/.version` (local/gitignored)
- RalphX.app expects a long-lived `0.x.y` line; multi-digit pre-1.0 versions such as `0.42.0` and `0.100.42` are valid.
- Generated notes should put user-facing changes first and developer/maintainer work in a separate `Developer And Maintainer Changes` section before the managed GitHub metadata block.
- `./scripts/propose-release.sh`, `./scripts/bump-version.sh`, `./scripts/generate-release-notes.sh`, and `./scripts/append-github-release-metadata.sh` still work as standalone lower-level steps
- Generated drafts should keep commit traceability as clickable Markdown links
- The public GitHub Release uses the full note, while app updater metadata strips the managed GitHub metadata block so in-app update notes stay concise.
- Codex generation logs are written to `.artifacts/release-notes/logs/`
- The full release sequence lives in `docs/release-process.md`
