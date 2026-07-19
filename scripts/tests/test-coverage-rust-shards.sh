#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${ROOT_DIR}/.github/workflows/coverage.yml"
# This value intentionally stays literal so the guard can match a GitHub expression.
# shellcheck disable=SC2016
MATRIX_PARTITION_EXPR='${{ matrix.partition }}'

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

rust_lib_job="$({
  awk '
    /^  rust-lib-coverage:/ { capture = 1 }
    /^  rust-ipc-coverage:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

rust_ipc_job="$({
  awk '
    /^  rust-ipc-coverage:/ { capture = 1 }
    /^  frontend-coverage:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

publish_job="$({
  awk '
    /^  publish-codecov:/ { capture = 1 }
    /^  coverage-status:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

[[ -n "${rust_lib_job}" ]] || fail "Rust lib coverage job is missing"
[[ -n "${rust_ipc_job}" ]] || fail "Rust IPC coverage job is missing"
[[ -n "${publish_job}" ]] || fail "Codecov publish job is missing"

for shard in 1 2 3 4; do
  grep -Fq "partition: \"${shard}/4\"" <<< "${rust_lib_job}" \
    || fail "Rust lib coverage is missing partition ${shard}/4"
  grep -Fq "artifact_suffix: \"${shard}\"" <<< "${rust_lib_job}" \
    || fail "Rust lib coverage is missing artifact suffix ${shard}"
  grep -Fq "coverage-artifacts/coverage-rust-lib-${shard}/lcov.info" <<< "${publish_job}" \
    || fail "Codecov publishing omits Rust lib coverage shard ${shard}"
done

grep -Fq -- "--partition hash:${MATRIX_PARTITION_EXPR}" <<< "${rust_lib_job}" \
  || fail "Rust lib coverage does not use deterministic matrix partitioning"

ipc_invocations="$(grep -Fc 'cargo llvm-cov nextest' <<< "${rust_ipc_job}")"
[[ "${ipc_invocations}" -eq 1 ]] \
  || fail "Rust IPC coverage must compile and execute through one llvm-cov nextest invocation"

for target in \
  suite_ipc_commands \
  suite_commands \
  suite_agent_workspace \
  suite_metrics \
  suite_ideation \
  suite_chat_service \
  suite_http_handlers \
  suite_interactive_process; do
  grep -Fq -- "--test ${target}" <<< "${rust_ipc_job}" \
    || fail "Rust IPC coverage omits ${target}"
done

for filter in \
  ipc_contract \
  release_notes_commands \
  agent_workspace_repair_auto_publish \
  agent_workspace_pr_review_notifications \
  metrics_commands \
  metrics_delivery_trends \
  metrics_integration \
  metrics_pr_insights \
  test_restart_ideation_implementation_core \
  persona; do
  grep -Fq "test(${filter})" <<< "${rust_ipc_job}" \
    || fail "Rust IPC coverage omits the ${filter} filter"
done

echo "PASS: Rust coverage uses four lib shards and one consolidated IPC build"
