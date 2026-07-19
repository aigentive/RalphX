#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${ROOT_DIR}/.github/workflows/ci.yml"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

integration_job="$({
  awk '
    /^  rust-full-integration:/ { capture = 1 }
    /^  rust-macos-build:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

[[ -n "${integration_job}" ]] || fail "Rust Full Integration job is missing"
grep -Fq 'CARGO_BUILD_JOBS: "1"' <<< "${integration_job}" \
  || fail "Rust Full Integration does not bound Cargo build concurrency"
grep -Fq 'CARGO_PROFILE_TEST_DEBUG: "0"' <<< "${integration_job}" \
  || fail "Rust Full Integration does not disable test-profile debug info"
grep -Fq 'heartbeat() {' <<< "${integration_job}" \
  || fail "Rust Full Integration does not start a heartbeat"
grep -Fq "printf '[rust-full-integration] %s still running\\n'" <<< "${integration_job}" \
  || fail "Rust Full Integration heartbeat does not emit progress"
grep -Fq 'sleep 60' <<< "${integration_job}" \
  || fail "Rust Full Integration heartbeat interval is not bounded"
grep -Fq 'trap cleanup EXIT' <<< "${integration_job}" \
  || fail "Rust Full Integration does not clean up its heartbeat"
grep -Fq -- '--partition hash:${{ matrix.partition }}' <<< "${integration_job}" \
  || fail "Rust Full Integration does not shard integration tests"

echo "PASS: Rust Full Integration bounds compile resources and shards integration targets"
