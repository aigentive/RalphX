#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
TEST_REPO="${TEST_TMP}/repo"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_equals() {
  local expected="$1"
  local actual="$2"
  [[ "${actual}" == "${expected}" ]] || fail "expected '${expected}', got '${actual}'"
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq -- "${expected}" "${file}" || fail "${file} is missing: ${expected}"
}

assert_resolver_fails() {
  local latest_tag="$1"
  local requested_tag="$2"
  local expected_message="$3"
  local output_file="${TEST_TMP}/resolver-error.txt"

  if (
    cd "${TEST_REPO}"
    # shellcheck disable=SC1091
    source "${ROOT_DIR}/scripts/release-analysis-common.sh"
    release_analysis_resolve_notes_from_tag "${latest_tag}" "${requested_tag}"
  ) >"${output_file}" 2>&1; then
    # shellcheck disable=SC2031
    fail "resolver unexpectedly accepted '${requested_tag}' relative to '${latest_tag}'"
  fi

  # shellcheck disable=SC2031
  grep -Fq -- "${expected_message}" "${output_file}" \
    || fail "resolver failure for '${requested_tag}' did not contain: ${expected_message}"
}

resolve_notes_from() {
  local latest_tag="$1"
  local requested_tag="$2"

  (
    cd "${TEST_REPO}"
    # shellcheck disable=SC1091
    source "${ROOT_DIR}/scripts/release-analysis-common.sh"
    release_analysis_resolve_notes_from_tag "${latest_tag}" "${requested_tag}"
  )
}

mkdir -p "${TEST_REPO}"
git -C "${TEST_REPO}" init -q -b main
git -C "${TEST_REPO}" config user.name "Release Test"
git -C "${TEST_REPO}" config user.email "release-test@example.com"

printf 'v0.69.0\n' >"${TEST_REPO}/history.txt"
git -C "${TEST_REPO}" add history.txt
git -C "${TEST_REPO}" commit -q -m "release v0.69.0"
git -C "${TEST_REPO}" tag v0.69.0

printf 'v0.70.0\n' >>"${TEST_REPO}/history.txt"
git -C "${TEST_REPO}" commit -q -am "release v0.70.0"
git -C "${TEST_REPO}" tag v0.70.0

printf 'next release\n' >>"${TEST_REPO}/history.txt"
git -C "${TEST_REPO}" commit -q -am "prepare next release"

assert_equals "v0.70.0" "$(resolve_notes_from "v0.70.0" "")"
assert_equals "v0.69.0" "$(resolve_notes_from "v0.70.0" "  v0.69.0  ")"
assert_equals "v0.70.0" "$(resolve_notes_from "v0.70.0" "v0.70.0")"

assert_resolver_fails "v0.70.0" "main" "must be an exact vX.Y.Z release tag"
assert_resolver_fails "v0.70.0" "v0.68.0" "does not exist"

git -C "${TEST_REPO}" switch -q -c unreachable v0.69.0
printf 'unreachable\n' >"${TEST_REPO}/unreachable.txt"
git -C "${TEST_REPO}" add unreachable.txt
git -C "${TEST_REPO}" commit -q -m "unreachable release"
git -C "${TEST_REPO}" tag v0.69.5
git -C "${TEST_REPO}" switch -q main
assert_resolver_fails "v0.70.0" "v0.69.5" "is not reachable from HEAD"

git -C "${TEST_REPO}" tag v0.71.0 HEAD
assert_resolver_fails "v0.70.0" "v0.71.0" "must be equal to or older than latest tag v0.70.0"

WORKFLOW_FILE="${ROOT_DIR}/.github/workflows/daily-release.yml"
assert_contains "${WORKFLOW_FILE}" "release_notes_from:"
release_notes_input_block="$(
  awk '
    /^      release_notes_from:/ { capture = 1; next }
    capture && /^      [a-z_]+:/ { exit }
    capture { print }
  ' "${WORKFLOW_FILE}"
)"
grep -Fq -- 'default: ""' <<<"${release_notes_input_block}" \
  || fail "release_notes_from input must default to an empty string"
# shellcheck disable=SC2016
assert_contains "${WORKFLOW_FILE}" 'MANUAL_RELEASE_NOTES_FROM: ${{ github.event_name == '\''workflow_dispatch'\'' && inputs.release_notes_from || '\'''\'' }}'
# shellcheck disable=SC2016
assert_contains "${WORKFLOW_FILE}" 'echo "analysis_from=${analysis_from}"'
# shellcheck disable=SC2016
assert_contains "${WORKFLOW_FILE}" '--from "${analysis_from}"'
# shellcheck disable=SC2016
assert_contains "${WORKFLOW_FILE}" '--current-version "${current_version}"'
# shellcheck disable=SC2016
assert_contains "${WORKFLOW_FILE}" '--previous-tag "${analysis_from}"'
assert_contains "${ROOT_DIR}/docs/release-process.md" 'release_notes_from'
assert_contains "${ROOT_DIR}/release-notes/README.md" 'release_notes_from'
if grep -Fq -- '-f "prerelease=true"' "${WORKFLOW_FILE}"; then
  fail "Daily Release must not dispatch the removed prerelease input"
fi
assert_contains "${ROOT_DIR}/.github/workflows/release.yml" 'raw_prerelease="true"'
assert_contains "${ROOT_DIR}/.github/workflows/release-publish.yml" 'Release Publish only accepts prerelease build metadata.'

echo "PASS: Daily Release notes-base override is validated and wired"
