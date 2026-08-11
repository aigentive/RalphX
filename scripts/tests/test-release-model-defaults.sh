#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXPECTED_MODEL="gpt-5.6-terra"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq -- "${expected}" "${file}" || fail "${file} is missing: ${expected}"
}

# shellcheck disable=SC2016
assert_contains "${ROOT_DIR}/scripts/propose-release.sh" \
  'DEFAULT_MODEL="${RELEASE_PROPOSAL_MODEL:-${RELEASE_NOTES_MODEL:-gpt-5.6-terra}}"'
# shellcheck disable=SC2016
assert_contains "${ROOT_DIR}/scripts/generate-release-notes.sh" \
  'DEFAULT_MODEL="${RELEASE_NOTES_MODEL:-gpt-5.6-terra}"'
assert_contains "${ROOT_DIR}/.github/workflows/daily-release.yml" \
  "default: ${EXPECTED_MODEL}"
assert_contains "${ROOT_DIR}/.github/workflows/daily-release.yml" \
  "RELEASE_PROPOSAL_MODEL: \${{ github.event_name == 'workflow_dispatch' && inputs.codex_model || '${EXPECTED_MODEL}' }}"
assert_contains "${ROOT_DIR}/.github/workflows/daily-release.yml" \
  "RELEASE_NOTES_MODEL: \${{ github.event_name == 'workflow_dispatch' && inputs.codex_model || '${EXPECTED_MODEL}' }}"
assert_contains "${ROOT_DIR}/docs/release-process.md" \
  "Scheduled runs use \`${EXPECTED_MODEL}\`"

"${ROOT_DIR}/scripts/propose-release.sh" --help \
  | grep -Fq "or ${EXPECTED_MODEL})" \
  || fail "proposal help does not report ${EXPECTED_MODEL}"
"${ROOT_DIR}/scripts/generate-release-notes.sh" --help \
  | grep -Fq "or ${EXPECTED_MODEL})" \
  || fail "release-notes help does not report ${EXPECTED_MODEL}"

echo "PASS: release Codex defaults are aligned on ${EXPECTED_MODEL}"
