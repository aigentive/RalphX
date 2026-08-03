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

- `Daily Release` runs from `main`, skips when there are no commits after the latest reachable `vX.Y.Z` tag, commits the generated `release-notes/vX.Y.Z.md` before tagging, and dispatches the prerelease-only Build/Publish path.
- Daily notes keep the curated RalphX summary first, then append a managed GitHub metadata block with pull-request attribution, first-time contributors when present, and the full changelog link.
- The scheduled workflow uses Codex CLI for both the version proposal and release-note generation, so the repository needs a `CODEX_API_KEY` secret.
- Protected-main setups may also need `RELEASE_AUTOMATION_TOKEN` with `contents:write` and `actions:write` so the workflow can push the release-prep commit/tag and dispatch `Release Build`.
- Manual `Daily Release` dispatch supports `dry_run=true` to verify generation without committing, tagging, pushing, or dispatching `Release Build`.
- Manual `Daily Release` dispatch supports `release_bump` to force `patch`, `minor`, or `major`; selecting `major` is explicit release-owner approval.
- Manual `Daily Release` dispatch also supports `release_version` to force an exact version; do not combine it with `release_bump`.
- Manual `Daily Release` dispatch supports `release_notes_from=<last-published-vX.Y.Z>` for recovery when a failed attempt left a newer tag without a release. Dry-run first; the override expands proposal/notes/metadata coverage but version selection remains based on the latest tag, which should not be deleted or rewritten.
- Manual `Daily Release` dispatch supports `linux_runner`, `arm_runner`, `intel_runner`, and `macos_runner_size` to choose the Release Build runner path; Blacksmith is the default for Linux prep/metadata/publish and both macOS builds, while `macos_runner_size=larger` intentionally uses paid GitHub-hosted macOS larger runners for GitHub-hosted macOS builds.
- `arm_runner` and `intel_runner` each support `blacksmith`, `depot`, `github-hosted`, and `self-hosted`. Blacksmith and Depot Intel builds use Apple Silicon runners with Tauri's `x86_64-apple-darwin` target.
- Daily Release, Release Build, and Release Publish always produce a published prerelease. Use `Stable Release Control` with `operation=promote` and `candidate_tag=<vX.Y.Z>`, or `operation=halt` with `bad_tag=<vX.Y.Z>` and the derived `restore_tag=<vX.Y.Z>`, to reconcile Stable GitHub authority, `updater-stable`, and Homebrew.
- Maintenance-only commits can avoid scheduled release prep when every commit after the latest tag includes `[skip daily-release]`, `[skip release]`, `[no daily-release]`, or `[no release]`.

Stable promotion notes:

- Files in this directory always stay the **per-build increment** for their own `vX.Y.Z` tag. Stable promotion never rewrites, regenerates, or adds a file here.
- `Stable Release Control` with `operation=promote` builds a **cumulative** note covering every build since the previous Stable release, using `./scripts/resolve-stable-baseline.sh` to find that baseline and `./scripts/generate-stable-release-notes.sh` to merge the committed per-build notes in `(last_stable, candidate]` with Codex.
- That combined note is release-hosted only: it replaces the promoted GitHub release body, the promoted release's `latest.json` notes, and both `updater-stable` pointer notes. It is also uploaded as a workflow artifact, but it is never committed back to the repository.
- A version tag with no committed note file here is skipped with a warning during combination, because the next build's note normally already covers those commits.
- Promote supports `notes_dry_run=true` to generate and upload the combined note without changing any release, pointer, or Homebrew state.
- `Stable Release Control` needs the same `CODEX_API_KEY` secret as `Daily Release`.

Notes:

- Release proposals default to `.artifacts/release-notes/proposal-from-v<current-version>.md`
- Accepted release versions are stored in `.artifacts/release-notes/.version` (local/gitignored)
- RalphX.app expects a long-lived `0.x.y` line; multi-digit pre-1.0 versions such as `0.42.0` and `0.100.42` are valid.
- Generated notes should put user-facing changes first and developer/maintainer work in a separate `Developer And Maintainer Changes` section before the managed GitHub metadata block.
- `./scripts/propose-release.sh`, `./scripts/bump-version.sh`, `./scripts/generate-release-notes.sh`, and `./scripts/append-github-release-metadata.sh` still work as standalone lower-level steps
- Generated drafts should keep commit traceability as clickable Markdown links
- The public GitHub Release uses the full note, while app updater metadata strips the managed GitHub metadata block so in-app update notes stay concise.
- Release Build uploads only app/DMG artifacts that pass final-DMG notarization, stapling, Gatekeeper, metadata, and matrix-architecture validation.
- Release Publish keeps versioned `latest.json` on the version release, then updates the exact `latest-aarch64.json` and `latest-x86_64.json` assets on fixed `updater-nightly` only after it has downloaded, validated, and publicly verified the complete prerelease assets.
- Codex generation logs are written to `.artifacts/release-notes/logs/`
- The full release sequence lives in `docs/release-process.md`
