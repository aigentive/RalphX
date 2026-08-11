#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
VERSION="v0.77.0"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq -- "${expected}" "${file}" || fail "${file} is missing: ${expected}"
}

assert_not_contains() {
  local file="$1"
  local unexpected="$2"
  ! grep -Fq -- "${unexpected}" "${file}" || fail "${file} must not contain: ${unexpected}"
}

assert_precedes() {
  local file="$1"
  local first="$2"
  local second="$3"
  local first_line second_line
  first_line="$(grep -nF -- "${first}" "${file}" | head -n1 | cut -d: -f1)"
  second_line="$(grep -nF -- "${second}" "${file}" | head -n1 | cut -d: -f1)"
  [[ -n "${first_line}" && -n "${second_line}" && "${first_line}" -lt "${second_line}" ]] \
    || fail "${file} must place ${first} before ${second}"
}

assert_fails() {
  local label="$1"
  shift

  if "$@" >"${TEST_TMP}/${label}.log" 2>&1; then
    fail "${label} unexpectedly succeeded"
  fi
}

NORMAL_DIR="${TEST_TMP}/normal"
CHANNEL_DIR="${TEST_TMP}/nightly"
STABLE_DIR="${TEST_TMP}/stable"
RENDER_DIR="${TEST_TMP}/rendered"
STABLE_RENDER_DIR="${TEST_TMP}/stable-rendered"
mkdir -p "${NORMAL_DIR}" "${CHANNEL_DIR}" "${STABLE_DIR}"

printf 'release notes\n' >"${TEST_TMP}/notes.md"
printf 'arm-signature-bytes' >"${TEST_TMP}/arm.sig"
printf 'intel-signature-bytes' >"${TEST_TMP}/intel.sig"

bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
  --tag "${VERSION}" \
  --notes-file "${TEST_TMP}/notes.md" \
  --aarch64-signature "${TEST_TMP}/arm.sig" \
  --x86_64-signature "${TEST_TMP}/intel.sig" \
  --pub-date '2026-07-23T10:00:00Z' \
  --channel nightly \
  --output-dir "${RENDER_DIR}"
bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
  --tag "${VERSION}" \
  --notes-file "${TEST_TMP}/notes.md" \
  --aarch64-signature "${TEST_TMP}/arm.sig" \
  --x86_64-signature "${TEST_TMP}/intel.sig" \
  --pub-date '2026-07-23T10:00:00Z' \
  --channel stable \
  --output-dir "${STABLE_RENDER_DIR}"

for arch in aarch64 x86_64; do
  printf '%s updater' "${arch}" >"${NORMAL_DIR}/RalphX_0.77.0_${arch}.app.tar.gz"
  if [[ "${arch}" == "aarch64" ]]; then
    cp "${TEST_TMP}/arm.sig" "${NORMAL_DIR}/RalphX_0.77.0_${arch}.app.tar.gz.sig"
  else
    cp "${TEST_TMP}/intel.sig" "${NORMAL_DIR}/RalphX_0.77.0_${arch}.app.tar.gz.sig"
  fi
  printf '%s dmg' "${arch}" >"${NORMAL_DIR}/RalphX_0.77.0_${arch}.dmg"
done
printf 'fixture checksums\n' >"${NORMAL_DIR}/checksums.txt"
cp "${RENDER_DIR}/latest.json" "${NORMAL_DIR}/latest.json"
cp "${RENDER_DIR}/latest-aarch64.json" "${CHANNEL_DIR}/latest-aarch64.json"
cp "${RENDER_DIR}/latest-x86_64.json" "${CHANNEL_DIR}/latest-x86_64.json"
cp "${STABLE_RENDER_DIR}/latest-aarch64.json" "${STABLE_DIR}/latest-aarch64.json"
cp "${STABLE_RENDER_DIR}/latest-x86_64.json" "${STABLE_DIR}/latest-x86_64.json"

bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" \
  "${VERSION}" "${NORMAL_DIR}" "${CHANNEL_DIR}" nightly
bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" \
  "${VERSION}" "${NORMAL_DIR}" "${STABLE_DIR}" stable

cp "${CHANNEL_DIR}/latest-aarch64.json" "${NORMAL_DIR}/latest-aarch64.json"
cp "${CHANNEL_DIR}/latest-x86_64.json" "${NORMAL_DIR}/latest-x86_64.json"
assert_fails rejects_pointer_assets_on_version_release \
  bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" "${VERSION}" "${NORMAL_DIR}" "${STABLE_DIR}" stable
rm "${NORMAL_DIR}/latest-aarch64.json" "${NORMAL_DIR}/latest-x86_64.json"

jq '.platforms["darwin-aarch64"].url = "https://example.invalid/asset"' \
  "${NORMAL_DIR}/latest.json" >"${TEST_TMP}/bad-latest.json"
mv "${TEST_TMP}/bad-latest.json" "${NORMAL_DIR}/latest.json"
assert_fails rejects_non_github_url \
  bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" "${VERSION}" "${NORMAL_DIR}" "${CHANNEL_DIR}" nightly
cp "${RENDER_DIR}/latest.json" "${NORMAL_DIR}/latest.json"

printf 'different-signature' >"${NORMAL_DIR}/RalphX_0.77.0_aarch64.app.tar.gz.sig"
assert_fails rejects_signature_mismatch \
  bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" "${VERSION}" "${NORMAL_DIR}" "${CHANNEL_DIR}" nightly
printf 'arm-signature-bytes' >"${NORMAL_DIR}/RalphX_0.77.0_aarch64.app.tar.gz.sig"

printf 'unexpected' >"${STABLE_DIR}/extra.txt"
assert_fails rejects_unallowlisted_stable_asset \
  bash "${ROOT_DIR}/scripts/validate-release-promotion.sh" "${VERSION}" "${NORMAL_DIR}" "${STABLE_DIR}" stable
rm "${STABLE_DIR}/extra.txt"

DAILY_WORKFLOW="${ROOT_DIR}/.github/workflows/daily-release.yml"
BUILD_WORKFLOW="${ROOT_DIR}/.github/workflows/release.yml"
PUBLISH_WORKFLOW="${ROOT_DIR}/.github/workflows/release-publish.yml"
PROMOTE_WORKFLOW="${ROOT_DIR}/.github/workflows/release-promote.yml"
CI_WORKFLOW="${ROOT_DIR}/.github/workflows/ci.yml"
STABLE_RECONCILER="${ROOT_DIR}/scripts/reconcile-stable-release-state.sh"

assert_not_contains "${DAILY_WORKFLOW}" '-f "prerelease=true"'
assert_contains "${BUILD_WORKFLOW}" 'raw_prerelease="true"'
assert_contains "${BUILD_WORKFLOW}" 'INPUT_REF: ${{ inputs.ref }}'
assert_not_contains "${BUILD_WORKFLOW}" 'raw_ref="${{ github.event.inputs.ref }}"'
assert_not_contains "${BUILD_WORKFLOW}" '--arg tag "${{ steps.meta.outputs.tag }}"'
assert_contains "${PUBLISH_WORKFLOW}" 'Release Publish only accepts prerelease build metadata.'
assert_contains "${PUBLISH_WORKFLOW}" 'fixed Nightly pointer release'
assert_contains "${PUBLISH_WORKFLOW}" 'reconcile-nightly-updater-pointers.sh'
assert_contains "${ROOT_DIR}/scripts/reconcile-nightly-updater-pointers.sh" 'updater-nightly'
assert_contains "${ROOT_DIR}/scripts/reconcile-nightly-updater-pointers.sh" 'latest-aarch64.json'
assert_not_contains "${PUBLISH_WORKFLOW}" 'HOMEBREW_TAP_TOKEN'
assert_not_contains "${PUBLISH_WORKFLOW}" 'Update Homebrew tap'
assert_contains "${PUBLISH_WORKFLOW}" '^[1-9][0-9]*$'
assert_contains "${PUBLISH_WORKFLOW}" 'release_name must equal RalphX.app'

assert_not_contains "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" 'gh '
assert_not_contains "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" 'curl '
assert_not_contains "${ROOT_DIR}/scripts/validate-release-promotion.sh" 'gh '
assert_not_contains "${ROOT_DIR}/scripts/validate-release-promotion.sh" 'git '
assert_not_contains "${ROOT_DIR}/scripts/validate-release-promotion.sh" 'curl '

assert_contains "${PROMOTE_WORKFLOW}" 'name: Stable Release Control'
assert_contains "${PROMOTE_WORKFLOW}" 'operation:'
assert_contains "${PROMOTE_WORKFLOW}" 'candidate_tag:'
assert_contains "${PROMOTE_WORKFLOW}" 'bad_tag:'
assert_contains "${PROMOTE_WORKFLOW}" 'restore_tag:'
assert_contains "${PROMOTE_WORKFLOW}" 'group: release-mutation-lane'
assert_contains "${PROMOTE_WORKFLOW}" 'cancel-in-progress: false'
assert_contains "${PROMOTE_WORKFLOW}" 'reconcile-stable-release-state.sh'
assert_contains "${PROMOTE_WORKFLOW}" 'reconcile-homebrew-cask.sh'
assert_contains "${STABLE_RECONCILER}" 'render-homebrew-cask.sh'
assert_contains "${STABLE_RECONCILER}" 'staged-homebrew-cask.rb'
assert_contains "${PROMOTE_WORKFLOW}" 'Preflight Homebrew credentials and tap access'
assert_contains "${PROMOTE_WORKFLOW}" 'git ls-remote --exit-code'
assert_precedes "${PROMOTE_WORKFLOW}" 'Preflight Homebrew credentials and tap access' 'Validate and reconcile requested Stable state'
assert_not_contains "${PROMOTE_WORKFLOW}" 'tauri build'
assert_not_contains "${PROMOTE_WORKFLOW}" 'git tag'
assert_not_contains "${PROMOTE_WORKFLOW}" 'gh workflow run'

assert_contains "${CI_WORKFLOW}" 'ACTIONLINT_VERSION="1.7.8"'
assert_contains "${CI_WORKFLOW}" 'be92c2652ab7b6d08425428797ceabeb16e31a781c07bc388456b4e592f3e36a'
assert_contains "${CI_WORKFLOW}" 'sha256sum --check --'
if grep -Eq '^[[:space:]]*actionlint([[:space:]]|$)' "${CI_WORKFLOW}"; then
  fail "${CI_WORKFLOW} must not invoke unresolved bare actionlint"
fi
assert_not_contains "${CI_WORKFLOW}" 'actionlint -color'
assert_not_contains "${STABLE_RECONCILER}" 'gh release list'
assert_contains "${STABLE_RECONCILER}" 'gh api --paginate --slurp'
assert_contains "${STABLE_RECONCILER}" '--latest=false'
assert_not_contains "${STABLE_RECONCILER}" 'isLatest'
assert_not_contains "${PUBLISH_WORKFLOW}" 'isLatest'
assert_not_contains "${PROMOTE_WORKFLOW}" 'isLatest'
assert_not_contains "${ROOT_DIR}/scripts/reconcile-nightly-updater-pointers.sh" 'updater-stable'
assert_contains "${ROOT_DIR}/scripts/tests/test-stable-release-state.sh" '[[ "$*" != *isLatest* ]]'

echo "PASS: release promotion contracts reject mutable or mismatched channel state"
