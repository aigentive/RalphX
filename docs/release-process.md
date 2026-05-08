# RalphX.app Release Process

This document covers the RalphX.app release workflow, from local build testing through public GitHub Releases, Homebrew publication, and in-app updater publication.

---

## Local Build Testing

### Build Without Signing (Development)

```bash
# Quick build for local testing (no signing)
cd frontend && npm run tauri build
```

Output:
- App: `src-tauri/target/release/bundle/macos/RalphX.app`
- DMG: `src-tauri/target/release/bundle/dmg/RalphX_*.dmg`

### Local Release-Like Build

Use the local helper when you want a release-mode build that still syncs local app data for internal testing:

```bash
./scripts/build-local-release.sh
```

This helper may seed the app-data DB from the dev DB and refresh plugin runtime into Application Support.

### Testing the Build

```bash
# Open the built app
open src-tauri/target/release/bundle/macos/RalphX.app

# Or mount and test the DMG
open src-tauri/target/release/bundle/dmg/RalphX_*.dmg
```

For signed builds, verify there are no Gatekeeper warnings when opening the app.

---

## Release Versioning Policy

RalphX.app is just starting formal public release management after an internal-only phase. The repo has very high development velocity and high code churn, so release versions follow the shipped product surface, not raw repository activity.

Current policy while RalphX.app remains on `0.x.y`:

| Bump | Use It When | Do Not Use It Just Because |
|---|---|---|
| `patch` | Fixes, polish, dependency churn, release/build/CI work, and internal changes that do not materially expand the shipped product surface | There were many commits, many changed files, a large diff stat, or a lot of release automation churn |
| `minor` | A release delivers a meaningful new user-visible capability or a meaningful expansion of an existing workflow | The product is still volatile or the team shipped a lot of internal work quickly |
| `major` | An explicit manually approved `1.0.0` milestone or a deliberate compatibility reset that deserves a public stability-contract change | Early-stage churn, broad refactors, large `0.x.y` numbers, or high release pressure |

Practical rules:

1. Public versioning tracks shipped behavior, install/update surface, and workflow shape.
2. Raw commit count, file count, diff size, dependency bump volume, and CI churn are supporting context only.
3. Frequent `minor` releases are acceptable in `0.x` if each release moves the visible product forward in a meaningful way.
4. `0.x.y` minor and patch numbers are unbounded integers; `0.42.0`, `0.100.0`, and `0.100.42` are valid pre-1.0 releases.
5. `1.0.0` is a deliberate product milestone, not an automatic consequence of high velocity, large minor numbers, or release pressure.
6. Automated Codex proposals must not advance the major version; a major bump requires explicit manual `release_bump=major` or `release_version=<major>` approval in `Daily Release`, or an equally explicit local release-owner action.

---

## Creating a Release

### Daily Scheduled Releases

`Daily Release` runs every day from `main` and releases committed changes when there are commits after the latest reachable `vX.Y.Z` tag.

Required repository secret:

- `CODEX_API_KEY` for Codex CLI release proposal and release-note generation. `OPENAI_API_KEY` is accepted as a fallback, but `CODEX_API_KEY` is preferred for `codex exec` automation.
- Optional: `RELEASE_AUTOMATION_TOKEN` with `contents:write` and `actions:write` when branch protection prevents the default `GITHUB_TOKEN` from pushing the release-prep commit/tag or dispatching `Release Build`.

What the scheduled workflow does:

1. Checks out `main` with tags.
2. Finds the latest reachable semver release tag.
3. Skips the run when there are no commits after that tag.
4. Installs Codex CLI with `npm i -g @openai/codex`.
5. Runs `./scripts/propose-release.sh --accept` for the version recommendation.
6. Runs `./scripts/bump-version.sh` and `./scripts/generate-release-notes.sh`.
7. Commits the version bump and `release-notes/vX.Y.Z.md` to `main`.
8. Tags that release-prep commit.
9. Dispatches `Release Build`, which still feeds the existing `Release Publish` workflow.

Manual testing:

1. Go to `aigentive/ralphx.app` -> Actions -> `Daily Release`.
2. Click **Run workflow** from `main`.
3. Use `dry_run=true` to verify Codex proposal, version bump, and note generation without committing, tagging, pushing, or dispatching the build.
4. Optionally set `release_bump` to force `patch`, `minor`, or `major`; `major` is the required approval path for a normal `1.0.0` jump.
5. Optionally set `release_version` to force an exact version such as `0.42.0`, `0.100.42`, or `v1.0.0`; do not combine it with `release_bump`.
6. Optionally set `linux_runner` to choose `blacksmith`, `depot`, `github-hosted`, or `self-hosted` for the Daily Release prep job and the dispatched Release Build metadata job.
7. Optionally set `arm_runner` to choose `blacksmith`, `depot`, `self-hosted`, or `github-hosted` for the macOS ARM build job.
8. Optionally set `intel_runner` to choose `blacksmith`, `depot`, `self-hosted`, or `github-hosted` for the macOS Intel build job. Blacksmith and Depot use Apple Silicon and cross-build the Intel artifact with the `x86_64-apple-darwin` target.
9. Optionally set `macos_runner_size=larger` to dispatch `Release Build` with paid larger GitHub-hosted macOS runners; the default `standard` uses standard runners.

Skipping scheduled release for maintenance-only commits:

- Add `[skip daily-release]`, `[skip release]`, `[no daily-release]`, or `[no release]` to every commit message after the latest release tag that should not produce a daily release.
- The scheduled workflow skips only when all commits after the latest reachable release tag carry one of those markers.
- Pushing to `main` can still run CI/CodeQL; this marker only affects the `Daily Release` workflow.

Scheduled runs use `gpt-5.5`, `draft=false`, `prerelease=false`, the Blacksmith Linux release runner, the Blacksmith macOS release runner, and standard GitHub-hosted macOS runner size by default. Manual dispatch can override those values.
If Codex proposes a major version without a manual bump/version override, the workflow fails before version bump, tag creation, build dispatch, or publish.

Runner labels:

- `linux_runner=blacksmith` → `blacksmith-32vcpu-ubuntu-2404`; `depot` → `depot-ubuntu-24.04`; `github-hosted` → `ubuntu-latest`; `self-hosted` → `["self-hosted","Linux","X64","ralphx-release"]`.
- `arm_runner=blacksmith` → `blacksmith-12vcpu-macos-15`; `depot` → `depot-macos-15`; `github-hosted` → `macos-15` or `macos-15-xlarge`; `self-hosted` → `["self-hosted","macOS","ARM64","ralphx-release"]`.
- `intel_runner=blacksmith` → `blacksmith-12vcpu-macos-15` with target `x86_64-apple-darwin`; `depot` → `depot-macos-15` with target `x86_64-apple-darwin`; `github-hosted` → `macos-15-intel` or `macos-15-large`; `self-hosted` → `["self-hosted","macOS","X64","ralphx-release"]`.
- Auto-triggered `Release Publish` runs on `blacksmith-32vcpu-ubuntu-2404`; manual publish dispatch can choose `blacksmith`, `depot`, `github-hosted`, or `self-hosted`.
- Blacksmith and Depot macOS runners are Apple Silicon, so Intel builds on those providers are cross-builds. GitHub-hosted and self-hosted Intel paths are native x86_64 runner paths.

---

### Preferred Flow: Guided Wrapper

Run the guided wrapper after the release code is finalized and local regression is green:

```bash
./scripts/release.sh
```

What it does:

1. Generates the release proposal
2. Pauses so you can review the proposal and accept or reject the suggested version
3. Stores the accepted version in `.artifacts/release-notes/.version`
4. Runs `./scripts/bump-version.sh`
5. Runs `./scripts/generate-release-notes.sh`
6. Pauses again so you can review and edit the generated artifacts before continuing to the manual git/tag/workflow steps

Primary review artifacts:

- proposal draft: `.artifacts/release-notes/proposal-from-v0.1.0.md`
- accepted version file: `.artifacts/release-notes/.version` (local/gitignored)
- release notes: `release-notes/vX.Y.Z.md`
- Codex logs: `.artifacts/release-notes/logs/`

Use `--from`, `--to`, `--current-version`, `--model`, or `--reasoning-effort` when you need to customize the compare range or Codex run.
Use `--allow-major` only when the release owner has explicitly approved accepting a proposed major version.

### Manual Flow

Use this when you want finer control than the wrapper gives you.

### Step 1: Propose The Version First

```bash
./scripts/propose-release.sh
```

Then:

1. Review the proposed bump (`patch` / `minor` / `major`) and the recommended version. Major proposals require explicit manual approval before they can be accepted.
2. Accept the proposal at the prompt if you want RalphX.app to store that version in `.artifacts/release-notes/.version`.
3. If you do not want the prompt, use:
   - `./scripts/propose-release.sh --accept`
4. If you reject the proposal, rerun with a different range or override the version manually in the next step.

Use `--from`, `--to`, or `--current-version` when you need to analyze a non-default compare range or when the current released version cannot be inferred from the start ref.

### Step 2: Bump The Chosen Version

If you accepted the proposal, you can omit the version:

```bash
./scripts/bump-version.sh
```

Or pass an explicit version if you are overriding:

```bash
./scripts/bump-version.sh 0.2.0
```

Explicit pre-1.0 versions can use multi-digit minor and patch numbers, for example `0.42.0` or `0.100.42`.

This updates version in:
- `frontend/package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

### Step 3: Commit Release Prep

```bash
git add frontend/package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "chore: bump version to 0.2.0"
```

Do not commit `.artifacts/release-notes/.version`; it is local state for the no-arg release helpers.

### Step 4: Draft And Review Release Notes

Run this after the version has been chosen and bumped, but before you push the release tag if you want the reviewed notes committed into `release-notes/vX.Y.Z.md` and picked up automatically by the release workflow.

If you accepted the proposal, you can omit the version here too:

```bash
./scripts/generate-release-notes.sh
```

Or pass an explicit version:

```bash
./scripts/generate-release-notes.sh 0.2.0
```

Then:

1. Review and edit the draft from:
   - `release-notes/v0.2.0.md`
2. Keep user-facing sections first and move developer, CI, docs, config, release automation, and scaffolding work into `Developer And Maintainer Changes` near the bottom.
3. If draft generation fails or you want to inspect the Codex run, check the logs in:
   - `.artifacts/release-notes/logs/`
4. Generated drafts include Markdown commit links for traceability; keep them clickable when editing notes.
5. Commit that curated notes file before tagging if you want the workflow-created draft GitHub release to use it automatically:
   - `git add release-notes/v0.2.0.md`
   - `git commit -m "docs: add release notes for v0.2.0"`
6. If you decide not to keep the draft in git, leave it uncommitted or remove it locally:
   - `rm -f release-notes/v0.2.0.md`

### Step 5: Create And Push The Release Tag

```bash
git tag v0.2.0
git push origin main --tags
```

### Step 6: Run The Release Build Workflow

After the tag is on `origin`, trigger `Release Build` manually from `main`:

1. Go to `aigentive/ralphx.app` → Actions → `Release Build`
2. Click **Run workflow**
3. Use:
   - `ref`: `v0.2.0`
   - `version`: `0.2.0`
   - `draft`: choose whether the public release should stay a draft
   - `prerelease`: choose whether the release should be marked as a prerelease
   - `linux_runner`: `blacksmith`, `depot`, `github-hosted`, or `self-hosted`
   - `arm_runner`: `blacksmith`, `depot`, `self-hosted`, or `github-hosted`
   - `intel_runner`: `blacksmith`, `depot`, `self-hosted`, or `github-hosted`
   - `macos_runner_size`: `standard` or `larger`; `larger` uses paid GitHub-hosted macOS larger runners for GitHub-hosted macOS release builds

Release Build passes an explicit Tauri target for each macOS artifact and collects bundles from the target-specific directory, for example `src-tauri/target/x86_64-apple-darwin/release`. Use `macos_runner_size=larger` when GitHub-hosted macOS jobs should use `macos-15-xlarge` for ARM or `macos-15-large` for Intel.

What `Release Build` does:

1. **Build**: Compiles frontend and Tauri app
2. **Sign**: Applies Developer ID certificate
3. **Notarize**: Submits to Apple for notarization
4. **Package**: Creates per-architecture DMGs and signed updater bundles
5. **Artifacts**: Uploads `release-aarch64`, `release-x86_64`, trace logs, and `release-metadata`
6. **Trigger**: A successful `Release Build` on `main` automatically triggers `Release Publish`

### Step 7: Verify The Publish Workflow

`Release Publish` reuses the successful build artifacts instead of rebuilding.

1. Go to `aigentive/ralphx.app` → Actions → `Release Publish`
2. Confirm the auto-triggered run finished successfully
3. For manual publish reruns, optionally set `linux_runner` to `depot`, `github-hosted`, or `self-hosted`; auto-triggered publish runs use Depot.
4. Then go to `aigentive/ralphx.app` → Releases
5. Find the release created or updated by the workflow
6. Review the artifacts:
   - `RalphX_x.x.x_aarch64.dmg` - Apple Silicon
   - `RalphX_x.x.x_x86_64.dmg` - Intel
   - `RalphX_x.x.x_aarch64.app.tar.gz` - Apple Silicon updater bundle
   - `RalphX_x.x.x_aarch64.app.tar.gz.sig` - Apple Silicon updater signature
   - `RalphX_x.x.x_x86_64.app.tar.gz` - Intel updater bundle
   - `RalphX_x.x.x_x86_64.app.tar.gz.sig` - Intel updater signature
   - `latest.json`
   - `checksums.txt`
6. Edit release notes as needed
7. If you dispatched the build with `draft=true`, click **Publish release**

## Manual Workflow Dispatch

For recovery publishing after a successful build run, use `Release Publish` manually instead of rebuilding:

1. Go to `aigentive/ralphx.app` → Actions → `Release Publish`
2. Click **Run workflow**
3. Provide:
   - `source_run_id`: the successful `Release Build` run ID
   - `ref`: `v0.2.0`
   - `version`: `0.2.0`
   - `draft` / `prerelease` flags to match the release you want
4. Click **Run workflow**

---

## In-App Updates

The release workflow publishes Tauri updater artifacts to the public source repo release.

Current release contract:
- updater endpoint: `https://github.com/aigentive/ralphx.app/releases/latest/download/latest.json`
- published releases include per-architecture `.app.tar.gz` updater bundles and `.sig` files
- `latest.json` points the app at those public updater bundles
- the updater follows GitHub's `latest` endpoint, so only the latest published non-draft release is visible automatically
- the Homebrew cask declares `auto_updates true`, so RalphX.app can self-update after install while still allowing an explicit `brew upgrade --cask ralphx`

---

## Homebrew Tap Publishing

The release workflow also maintains the public tap repo `aigentive/homebrew-ralphx`.

Current tap contract:
- release artifacts stay in `aigentive/ralphx.app`
- `Casks/ralphx.rb` is rendered from the release workflow using the per-arch DMG sha256 values
- only non-draft, non-prerelease releases update the tap automatically
- testers install with `brew tap aigentive/ralphx` and `brew install --cask ralphx`

---

## Troubleshooting

### Build Failures

**"No signing identity found"**
```bash
# Verify certificate is installed
security find-identity -v -p codesigning

# Should show:
# "Developer ID Application: Your Name (TEAM_ID)"
```

**"Unable to notarize"**
- Verify `APPLE_API_ISSUER`, `APPLE_API_KEY`, and `APPLE_API_KEY_P8`
- Ensure `APPLE_TEAM_ID` matches the signing team
- Check Apple's notarization service status at [developer.apple.com/system-status](https://developer.apple.com/system-status/)

**Cargo build errors**
```bash
# Clean and rebuild
cd src-tauri
cargo clean
cargo tauri build
```

### GitHub Actions Issues

**"Secret not found"**
- Verify all secrets are configured in repository settings
- Secret names are case-sensitive
- Homebrew tap publishing also requires `HOMEBREW_TAP_TOKEN`

**"Certificate import failed"**
- Re-export the certificate and base64 encode it
- Verify the password matches `APPLE_CERTIFICATE_PASSWORD`

**Workflow doesn't trigger**
- Ensure tag follows pattern `v*` (e.g., `v0.2.0`)
- `Release Publish` auto-triggers only after a successful `Release Build` run from `main`
- Check the Actions tab for `Release Build` and `Release Publish`

**Public release upload failed**
- Verify the workflow has `contents: write` permission for `aigentive/ralphx.app`
- Confirm the tag exists and the GitHub Actions token can create or update releases

**Homebrew tap update failed**
- Verify `HOMEBREW_TAP_TOKEN` has `Contents: Read and write` on `aigentive/homebrew-ralphx`
- Confirm the tap repo exists, is public, and contains a top-level `Casks/` directory

**Updater assets missing**
- Confirm `src-tauri/tauri.conf.json` still has `"bundle.createUpdaterArtifacts": true`
- Confirm the build produced `.app.tar.gz` and `.app.tar.gz.sig` files under `src-tauri/target/release/bundle/macos/`

### Gatekeeper Issues

**"App is damaged and can't be opened"**
- App wasn't properly signed or notarized
- Check the release workflow logs for signing/notarization errors
- For local testing, temporarily allow: `xattr -cr /path/to/RalphX.app`

**"Developer cannot be verified"**
- Notarization may not have completed
- Check [developer.apple.com/system-status](https://developer.apple.com/system-status/)
- Wait a few minutes and try again

---

## File Reference

| File | Purpose |
|------|---------|
| `.github/workflows/release.yml` | Build-only release workflow: sign, notarize, package, and upload release artifacts |
| `.github/workflows/release-publish.yml` | Publish workflow: consume release artifacts, publish public assets, and update Homebrew |
| `scripts/build-local-release.sh` | Local internal release-like build script |
| `scripts/build-prod-release.sh` | Internal CI release artifact entrypoint |
| `scripts/release.sh` | Guided local release-prep wrapper that orchestrates proposal, version bump, and release-note generation |
| `scripts/propose-release.sh` | Codex-assisted version recommendation generator |
| `scripts/release-analysis-common.sh` | Shared release evidence and Codex logging helper used by the proposal and notes scripts |
| `scripts/bump-version.sh` | Version management script |
| `scripts/generate-release-notes.sh` | Codex-assisted release notes draft generator |
| `release-notes/` | Curated release notes consumed automatically by the release workflow when present |
| `src-tauri/tauri.conf.json` | Bundle config, updater config |
| `src-tauri/Cargo.toml` | Release profile, updater dependency |
| `src-tauri/entitlements.plist` | Hardened runtime entitlements |
| `src/components/UpdateChecker.tsx` | Update notification UI |
