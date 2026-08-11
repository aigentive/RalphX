# RalphX.app Release Process

This document covers the RalphX.app release workflow: versioned prerelease builds, explicit Stable control, Homebrew publication, and in-app updater manifests.

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

Release artifacts can be inspected without submitting or publishing them:

```bash
./scripts/validate-macos-release-artifacts.sh \
  --app src-tauri/target/aarch64-apple-darwin/release/bundle/macos/RalphX.app \
  --dmg /absolute/path/to/RalphX_0.67.0_aarch64.dmg \
  --expected-arch aarch64
```

Pass one exact app, one exact DMG, and the matrix architecture (`aarch64` or `x86_64`). The validator is read-only: it checks signatures, stapled tickets, Gatekeeper policy, app metadata, and executable architecture without submitting to Apple or removing quarantine.

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

- `CODEX_API_KEY` for Codex CLI release proposal and release-note generation. `OPENAI_API_KEY` is accepted as a fallback, but `CODEX_API_KEY` is preferred for `codex exec` automation. `Stable Release Control` uses the same secret for cumulative Stable release notes.
- Optional: `RELEASE_AUTOMATION_TOKEN` with `contents:write` and `actions:write` when branch protection prevents the default `GITHUB_TOKEN` from pushing the release-prep commit/tag or dispatching `Release Build`.

What the scheduled workflow does:

1. Checks out `main` with tags.
2. Finds the latest reachable semver release tag.
3. Skips the run when there are no commits after that tag.
4. Installs Codex CLI with `npm i -g @openai/codex`.
5. Runs `./scripts/propose-release.sh --accept` for the version recommendation.
6. Runs `./scripts/bump-version.sh`, `./scripts/generate-release-notes.sh`, and `./scripts/append-github-release-metadata.sh`.
7. Commits the version bump and `release-notes/vX.Y.Z.md` to `main`.
8. Tags that release-prep commit.
9. Dispatches `Release Build`, which feeds `Release Publish` as a published prerelease only.

The committed release note keeps the curated RalphX summary first, then appends a managed GitHub metadata block with pull-request attribution, new contributors when present, and the full changelog link. The public GitHub Release uses the full note. The app updater metadata strips that managed block so in-app update notes stay concise.

Manual testing:

1. Go to `aigentive/ralphx.app` -> Actions -> `Daily Release`.
2. Click **Run workflow** from `main`.
3. Use `dry_run=true` to verify Codex proposal, version bump, and note generation without committing, tagging, pushing, or dispatching the build.
4. Optionally set `release_bump` to force `patch`, `minor`, or `major`; `major` is the required approval path for a normal `1.0.0` jump.
5. Optionally set `release_version` to force an exact version such as `0.42.0`, `0.100.42`, or `v1.0.0`; do not combine it with `release_bump`.
6. For failed-tag recovery, optionally set `release_notes_from` to the last published `vX.Y.Z` tag. This changes the proposal, curated notes, and GitHub metadata range without changing the version base.
7. Optionally set `linux_runner` to choose `blacksmith`, `depot`, `github-hosted`, or `self-hosted` for the Daily Release prep job and the dispatched Release Build metadata job.
8. Optionally set `arm_runner` to choose `blacksmith`, `depot`, `self-hosted`, or `github-hosted` for the macOS ARM build job.
9. Optionally set `intel_runner` to choose `blacksmith`, `depot`, `self-hosted`, or `github-hosted` for the macOS Intel build job. Blacksmith and Depot use Apple Silicon and cross-build the Intel artifact with the `x86_64-apple-darwin` target.
10. Optionally set `macos_runner_size=larger` to dispatch `Release Build` with paid larger GitHub-hosted macOS runners; the default `standard` uses standard runners.

Scheduled runs and manual runs with a blank `release_notes_from` use the latest reachable release tag for both version selection and release notes. When the override is set, version selection and release-worthiness still use the latest reachable tag; only release analysis and notes start from the validated older tag.

Skipping scheduled release for maintenance-only commits:

- Add `[skip daily-release]`, `[skip release]`, `[no daily-release]`, or `[no release]` to every commit message after the latest release tag that should not produce a daily release.
- The scheduled workflow skips only when all commits after the latest reachable release tag carry one of those markers.
- Pushing to `main` can still run CI/CodeQL; this marker only affects the `Daily Release` workflow.

Scheduled runs use `gpt-5.6-terra`, a published prerelease (`draft=false`, `prerelease=true`), the Blacksmith Linux release runner, the Blacksmith macOS release runner, and standard GitHub-hosted macOS runner size. Stable state is never selectable from Daily Release.
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
5. Runs `./scripts/generate-release-notes.sh` and `./scripts/append-github-release-metadata.sh`
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

Append GitHub pull-request and contributor metadata before reviewing the final public note:

```bash
./scripts/append-github-release-metadata.sh \
  --tag v0.2.0 \
  --previous-tag v0.1.0 \
  --target HEAD \
  --notes-file release-notes/v0.2.0.md
```

Then:

1. Review and edit the draft from:
   - `release-notes/v0.2.0.md`
2. Keep user-facing sections first and move developer, CI, docs, config, release automation, and scaffolding work into `Developer And Maintainer Changes` near the bottom.
3. If draft generation fails or you want to inspect the Codex run, check the logs in:
   - `.artifacts/release-notes/logs/`
4. Generated drafts include Markdown commit links for traceability; keep them clickable when editing notes.
5. Keep the managed GitHub metadata block at the end unless you intentionally rerun `./scripts/append-github-release-metadata.sh`.
6. Commit that curated notes file before tagging if you want the workflow-created draft GitHub release to use it automatically:
   - `git add release-notes/v0.2.0.md`
   - `git commit -m "docs: add release notes for v0.2.0"`
7. If you decide not to keep the draft in git, leave it uncommitted or remove it locally:
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
   - `linux_runner`: `blacksmith`, `depot`, `github-hosted`, or `self-hosted`
   - `arm_runner`: `blacksmith`, `depot`, `self-hosted`, or `github-hosted`
   - `intel_runner`: `blacksmith`, `depot`, `self-hosted`, or `github-hosted`
   - `macos_runner_size`: `standard` or `larger`; `larger` uses paid GitHub-hosted macOS larger runners for GitHub-hosted macOS release builds

Release Build passes an explicit Tauri target for each macOS artifact and collects bundles from the target-specific directory, for example `src-tauri/target/x86_64-apple-darwin/release`. Use `macos_runner_size=larger` when GitHub-hosted macOS jobs should use `macos-15-xlarge` for ARM or `macos-15-large` for Intel.

What `Release Build` does:

1. **Build**: Compiles frontend and Tauri app
2. **Sign**: Applies Developer ID certificate
3. **Notarize app**: Submits the app to Apple and staples its accepted ticket
4. **Package**: Creates per-architecture DMGs and signed updater bundles
5. **Notarize DMG**: Submits and staples the finished, signed DMG
6. **Validate**: Blocks artifact collection unless the app and DMG pass signature, ticket, Gatekeeper, metadata, and architecture policy checks
7. **Artifacts**: Uploads only validated `release-aarch64`, `release-x86_64`, trace logs, and `release-metadata`
8. **Trigger**: A successful `Release Build` on `main` automatically triggers `Release Publish` for a published prerelease

The DMG is validated after stapling, so downstream checksums and publication always use the final byte-level deliverable. Local validation on the current macOS 26 host is supported; no clean VM is required. A notarization rejection, timeout, missing ticket, wrong architecture, or Gatekeeper failure stops the build before artifact collection.

### Step 7: Verify The Publish Workflow

`Release Publish` reuses the successful build artifacts instead of rebuilding.

1. Go to `aigentive/ralphx.app` → Actions → `Release Publish` and confirm the auto-triggered run finished successfully.
2. The release must remain published and marked as a prerelease; a normal publish run cannot create a Stable release or update Homebrew.
3. Then go to `aigentive/ralphx.app` → Releases and find the release created or updated by the workflow.
4. Review the fixed artifact set:
   - `RalphX_x.x.x_aarch64.dmg` - Apple Silicon
   - `RalphX_x.x.x_x86_64.dmg` - Intel
   - `RalphX_x.x.x_aarch64.app.tar.gz` - Apple Silicon updater bundle
   - `RalphX_x.x.x_aarch64.app.tar.gz.sig` - Apple Silicon updater signature
   - `RalphX_x.x.x_x86_64.app.tar.gz` - Intel updater bundle
   - `RalphX_x.x.x_x86_64.app.tar.gz.sig` - Intel updater signature
   - `latest.json`
   - `checksums.txt`
5. `Release Publish` first verifies that the eight versioned source assets are readable and their signatures and fixed GitHub tag URLs match `latest.json`; only then does it upload and publicly verify the two fixed `updater-nightly` architecture pointers.

## Manual Workflow Dispatch

For recovery publishing after a successful build run, use `Release Publish` manually instead of rebuilding:

1. Go to `aigentive/ralphx.app` → Actions → `Release Publish`
2. Click **Run workflow**
3. Provide:
   - `source_run_id`: the successful `Release Build` run ID
   - `ref`: `v0.2.0`
   - `version`: `0.2.0`
4. Click **Run workflow**

Manual Publish uses the build run's persisted `draft=false` and `prerelease=true` metadata. It rejects any build metadata that could create or edit Stable state.

### Step 8: Promote Or Halt Stable

`Stable Release Control` is the sole workflow that can change Stable GitHub authority, the fixed `updater-stable` pointers, or the Homebrew cask. It never builds, signs, re-signs, creates, moves, or pushes a release tag.

GitHub Immutable Releases must remain disabled for this no-rebuild channel design: Stable promotion reclassifies version releases and both fixed updater releases replace same-named assets. The workflows inspect the Release API `immutable` field and fail closed; enabling immutable releases makes this promotion mechanism unavailable and requires a different transport/process.

To promote a tested prerelease:

1. Go to `aigentive/ralphx.app` → Actions → `Stable Release Control`.
2. Choose `operation=promote` and set only `candidate_tag` to the exact source tag, for example `v0.77.0` (not `0.77.0`, a branch, or a ref prefix).
3. Before any mutation, the workflow builds the cumulative Stable release note (see below).
4. The workflow validates/downloads/renders the candidate: it requires the exact eight-source-asset allowlist, byte-compares manifest signatures with their `.sig` files, checks every updater URL is the fixed GitHub download URL for that tag, and stages the deterministic Homebrew cask from the downloaded DMGs.
5. It then creates or validates the mutable, published-prerelease `updater-stable` infrastructure release before changing version authority; only after that does it make the candidate the full GitHub latest release, apply the combined notes, publish both fixed pointers, wait for their public URLs to converge, and reconcile the already-staged cask.
6. Later promotions are semver-monotonic, but older successful Stable releases remain published full releases as history; normal promotion never demotes them to prereleases.

An exact rerun repairs only bounded partial states for the same request: GitHub authority already advanced with absent, prior, or one-architecture-updated pointers; or completed pointers with an unfinished Homebrew cask. Unrelated pointer disagreement, a draft/immutable release, a mismatched full release, or a missing fixed asset fails closed. Reruns regenerate the combined note and reapply it idempotently.

#### Cumulative Stable Release Notes

Stable promotion bundles every prerelease build since the previous Stable release, so a Stable upgrader never saw the intermediate per-build notes. Promotion therefore replaces the promoted release's presentation with one combined note covering the whole span. It does not rebuild, retag, or copy assets.

Generation runs as a read-only pre-step, before any release, pointer, or Homebrew mutation:

1. `scripts/resolve-stable-baseline.sh` resolves the previous Stable tag — the newest published full `vX.Y.Z` release strictly below `candidate_tag`. If none exists (first-ever promotion), the whole combine path is skipped with a summary line and promotion proceeds with the candidate's existing per-build note.
2. `scripts/generate-stable-release-notes.sh` collects the committed `release-notes/vX.Y.Z.md` for every version tag in `(last_stable, candidate]`, reading them from the candidate tag's own tree, strips each managed metadata block, and drives Codex to merge and dedupe them into one note. Version tags with no committed note are skipped with a warning; the next build's note normally already covers them.
3. `scripts/append-github-release-metadata.sh` appends a fresh metadata block spanning `last_stable...candidate`, so pull-request attribution and the full changelog link cover the whole Stable span. A stripped copy is produced for the updater surfaces.
4. Both files are uploaded as the `stable-release-notes-<candidate_tag>` workflow artifact.

`scripts/reconcile-stable-release-state.sh` then applies them, only after GitHub authority has advanced to the candidate:

- the promoted release **body** is replaced with the combined note, which is also what the in-app "What's New" view shows
- the promoted release's `latest.json` is re-rendered with the stripped combined note and clobbered in place, so legacy clients on `/releases/latest/download/latest.json` and any later halt recovery stay consistent
- both `updater-stable` pointers carry the same stripped combined note

Only the `notes` field changes; version, fixed download URLs, and signature bytes are re-rendered identically from the same source assets and re-verified afterwards. The committed `release-notes/vX.Y.Z.md` files are never rewritten, and the promote workflow makes no git commit.

Promote-only inputs:

| Input | Default | Purpose |
|---|---|---|
| `codex_model` | `gpt-5.6-terra` | Codex model for the combine step |
| `codex_reasoning_effort` | `xhigh` | `low`, `medium`, `high`, or `xhigh` |
| `notes_dry_run` | `false` | Generate and upload the combined note, then stop before every mutation |

Use `notes_dry_run=true` to review the generated note as a rehearsal before a real promotion; it changes no release, pointer, or Homebrew state. The gate applies to `promote` only and can never block a `halt`.

`Stable Release Control` now needs the `CODEX_API_KEY` secret (with `OPENAI_API_KEY` as a fallback) that `Daily Release` already uses. A missing key or a Codex failure fails the run during generation, before anything has been mutated.

To halt Stable, select `operation=halt`, set `bad_tag` to the exact current Stable tag, and set `restore_tag` to the exact derived prior full Stable tag. The workflow validates the bad and restore releases first, demotes the bad release and promotes the restore release in GitHub, restores both `updater-stable` pointers, and then reconciles Homebrew. Exact reruns repair only those bad/restore partial states; they never accept another candidate combination. It does not build, upload versioned assets, retag, or push a release tag.

A halt stops new Stable and Homebrew delivery; it does not downgrade already-installed clients. Those clients remain on their installed version and wait for a later fixed Stable promotion with a newer version.

---

## In-App Updates

The release workflow publishes Tauri updater artifacts to the public source repo release.

Current release contract:
- every versioned prerelease retains only its normal `latest.json`, with both Tauri macOS targets and fixed URLs under its own `vX.Y.Z` GitHub Release tag
- `updater-nightly` owns the two fixed one-target Nightly pointers: `latest-aarch64.json` and `latest-x86_64.json`; it is always a published prerelease with `latest=false`, and each pointer is rendered only after the versioned source assets are readable and validated
- `updater-stable` owns the same two fixed one-target Stable pointers; it is always a published prerelease with `latest=false`, and Stable control first moves the versioned GitHub latest authority, then replaces both pointers and verifies the public pointer bytes before continuing
- Nightly pointer notes are the per-build note for that version; Stable pointer notes are the cumulative note since the previous Stable release, so the Stable updater dialog describes the whole upgrade span rather than the last daily increment
- the Homebrew cask declares `auto_updates true`, so RalphX.app can self-update after install while still allowing an explicit `brew upgrade --cask ralphx`

Rollout compatibility: already-shipped clients using `/releases/latest/download/latest.json` remain Stable-only because `Release Publish` keeps normal `latest.json` on every version release and only Stable Release Control makes a version release GitHub latest. New builds use `updater-{{target}}/latest-{{arch}}.json`.

---

## Homebrew Tap Publishing

Only `Stable Release Control` maintains the public tap repo `aigentive/homebrew-ralphx`.

Current tap contract:
- release artifacts stay in `aigentive/ralphx.app`
- `Casks/ralphx.rb` is rendered by `scripts/render-homebrew-cask.sh` using the authoritative Stable release's per-arch DMG sha256 values
- Daily, Build, and Publish never update the tap; both exact Stable promotion and halt reruns reconcile it after GitHub and `updater-stable` authority converge
- testers install with `brew tap aigentive/ralphx` and `brew install --cask ralphx`

---

## Troubleshooting

### Failed Daily Release Left a Tag Without a GitHub Release

If a failed release attempt pushed a tag but did not publish its GitHub Release, keep that tag and recover through a manual `Daily Release` run:

1. Run with `dry_run=true`, `release_notes_from` set to the last published release tag (for example, `v0.69.0`), and `release_bump=auto` unless a specific bump or version is required.
2. Confirm the dry-run proposal and generated note cover the cumulative range from that published tag through `HEAD`, while the proposed version is newer than the failed tag.
3. Re-run without dry run using the same `release_notes_from` value.

Do not delete, move, or rewrite the failed tag during normal recovery. The workflow treats it as an occupied version while using the older published tag only as the release-notes comparison base.

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
- Check the release trace for separate app/DMG submission, acceptance, stapling, and policy-validation stages

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
- Stable Homebrew cask publishing requires `HOMEBREW_TAP_TOKEN`

**"Certificate import failed"**
- Re-export the certificate and base64 encode it
- Verify the password matches `APPLE_CERTIFICATE_PASSWORD`

**Workflow doesn't trigger**
- Ensure tag follows pattern `v*` (e.g., `v0.2.0`)
- `Release Publish` auto-triggers only after a successful `Release Build` run from `main`
- Check the Actions tab for `Release Build`, `Release Publish`, and (when deliberately promoting) `Stable Release Control`

**Public release upload failed**
- Verify the workflow has `contents: write` permission for `aigentive/ralphx.app`
- Confirm the tag exists and the GitHub Actions token can create or update releases

**Stable Homebrew cask update failed**
- Verify `HOMEBREW_TAP_TOKEN` has `Contents: Read and write` on `aigentive/homebrew-ralphx`
- Confirm the tap repo exists, is public, and contains a top-level `Casks/` directory

**Updater assets missing**
- Confirm `src-tauri/tauri.conf.json` still has `"bundle.createUpdaterArtifacts": true`
- Confirm the build produced `.app.tar.gz` and `.app.tar.gz.sig` files under `src-tauri/target/release/bundle/macos/`

### Gatekeeper Issues

**"App is damaged and can't be opened"**
- The app or enclosing DMG may not have passed signing, notarization, stapling, or Gatekeeper policy checks
- Run `scripts/validate-macos-release-artifacts.sh` against the exact downloaded artifacts and check the release trace
- Quarantine removal is not an installation fix. If needed for diagnosis, use `xattr -cr` only on a disposable copy and compare behavior; never modify the installed production app as routine remediation.

**"Developer cannot be verified"**
- Notarization may not have completed
- Check [developer.apple.com/system-status](https://developer.apple.com/system-status/)
- Wait a few minutes and try again

---

## File Reference

| File | Purpose |
|------|---------|
| `.github/workflows/release.yml` | Build-only release workflow: sign, notarize, package, and upload release artifacts |
| `.github/workflows/release-publish.yml` | Prerelease-only publish workflow: consume validated build artifacts, publish versioned source assets, and reconcile `updater-nightly` pointers |
| `.github/workflows/release-promote.yml` | Stable-control workflow: validates/promotes or halts with ordered versioned GitHub authority → `updater-stable` pointers → Homebrew reconciliation |
| `scripts/build-local-release.sh` | Local internal release-like build script |
| `scripts/build-prod-release.sh` | Internal CI release artifact entrypoint |
| `scripts/render-updater-channel-manifests.sh` | Pure renderer for versioned and one-target channel Tauri updater manifests with fixed GitHub tag URLs |
| `scripts/validate-release-promotion.sh` | Pure fail-closed validator for fixed release asset sets, manifest URLs, and signature bytes |
| `scripts/reconcile-stable-release-state.sh` | Ordered Stable promote/halt state machine with bounded exact-rerun recovery |
| `scripts/reconcile-homebrew-cask.sh` | Idempotently commits, pushes, and byte-verifies the pre-rendered Stable cask after Stable authority convergence |
| `scripts/verify-public-updater-pointers.sh` | Bounded public GitHub pointer-cache verification shared by Stable and Nightly publication |
| `scripts/validate-macos-release-artifacts.sh` | Read-only app/DMG signature, ticket, Gatekeeper, metadata, and architecture validator |
| `scripts/release.sh` | Guided local release-prep wrapper that orchestrates proposal, version bump, and release-note generation |
| `scripts/propose-release.sh` | Codex-assisted version recommendation generator |
| `scripts/release-analysis-common.sh` | Shared release evidence and Codex logging helper used by the proposal and notes scripts |
| `scripts/bump-version.sh` | Version management script |
| `scripts/generate-release-notes.sh` | Codex-assisted per-build release notes draft generator |
| `scripts/generate-stable-release-notes.sh` | Codex-assisted combiner that merges the committed per-build notes since the previous Stable release into one cumulative Stable note |
| `scripts/resolve-stable-baseline.sh` | Resolves the newest published full `vX.Y.Z` release strictly below a ceiling tag; empty output means first-ever promotion |
| `scripts/prompts/stable-release-notes-codex-prompt.md` | Model instructions for merging and deduping per-build notes into one Stable note |
| `scripts/append-github-release-metadata.sh` | Appends managed GitHub pull-request, contributor, and changelog metadata to release notes |
| `release-notes/` | Curated release notes consumed automatically by the release workflow when present |
| `src-tauri/tauri.conf.json` | Bundle config, updater config |
| `src-tauri/Cargo.toml` | Release profile, updater dependency |
| `src/components/UpdateChecker.tsx` | Update notification UI |
