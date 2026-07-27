#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
VERSION="v0.77.0"
RELEASE_BASE="https://github.com/aigentive/ralphx.app/releases/download/${VERSION}"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_file() {
  [[ -f "$1" ]] || fail "missing file: $1"
}

assert_target_manifest() {
  local manifest="$1"
  local target="$2"
  local archive="$3"
  local signature="$4"

  assert_file "${manifest}"
  jq -e --arg target "${target}" --arg archive "${archive}" --arg signature "${signature}" '
    (keys | sort) == ["notes", "platforms", "pub_date", "version"]
    and .version == "v0.77.0"
    and (.platforms | keys) == [$target]
    and .platforms[$target].url == $archive
    and .platforms[$target].signature == $signature
  ' "${manifest}" >/dev/null || fail "${manifest} is not an exact ${target} Tauri target manifest"
}

NOTES_FILE="${TEST_TMP}/notes.md"
ARM_SIG="${TEST_TMP}/arm.sig"
INTEL_SIG="${TEST_TMP}/intel.sig"
OUTPUT_DIR="${TEST_TMP}/rendered"

printf 'A concise release note.\n' >"${NOTES_FILE}"
printf 'arm-signature-bytes' >"${ARM_SIG}"
printf 'intel-signature-bytes' >"${INTEL_SIG}"

bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
  --tag "${VERSION}" \
  --notes-file "${NOTES_FILE}" \
  --aarch64-signature "${ARM_SIG}" \
  --x86_64-signature "${INTEL_SIG}" \
  --pub-date '2026-07-23T10:00:00Z' \
  --channel nightly \
  --output-dir "${OUTPUT_DIR}"

assert_file "${OUTPUT_DIR}/latest.json"
assert_target_manifest \
  "${OUTPUT_DIR}/latest-aarch64.json" \
  'nightly' \
  "${RELEASE_BASE}/RalphX_0.77.0_aarch64.app.tar.gz" \
  'arm-signature-bytes'
assert_target_manifest \
  "${OUTPUT_DIR}/latest-x86_64.json" \
  'nightly' \
  "${RELEASE_BASE}/RalphX_0.77.0_x86_64.app.tar.gz" \
  'intel-signature-bytes'

jq -e '
  (.platforms | keys | sort) == ["darwin-aarch64", "darwin-x86_64"]
  and .platforms["darwin-aarch64"].signature == "arm-signature-bytes"
  and .platforms["darwin-x86_64"].signature == "intel-signature-bytes"
' "${OUTPUT_DIR}/latest.json" >/dev/null || fail "latest.json must retain both exact updater targets"

echo "PASS: updater channel manifests render exact Tauri targets"
