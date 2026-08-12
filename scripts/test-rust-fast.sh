#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if REPO_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
fi
MANIFEST_PATH="${REPO_ROOT}/src-tauri/Cargo.toml"
FAST_TARGET_ROOT="${REPO_ROOT}/src-tauri/target/rust-fast"
FULL_INTEGRATION_TESTS=(
  suite_agent_workspace
  suite_chat_service
  suite_commands
  suite_http_handlers
  suite_ideation
  suite_interactive_process
  suite_ipc_commands
  suite_metrics
  suite_pr_github
  suite_sqlite_flows
  suite_sqlite_repos
  suite_transition_git
  tauri_events
  plan_selector_performance
)

CURRENT_TOPLEVEL="$(git -C "${PWD}" rev-parse --show-toplevel 2>/dev/null || true)"

if [[ -n "${CURRENT_TOPLEVEL}" && "${CURRENT_TOPLEVEL}" != "${REPO_ROOT}" ]]; then
  printf '[rust-fast] Refusing to run from %s because this script belongs to %s.\n' "${CURRENT_TOPLEVEL}" "${REPO_ROOT}" >&2
  printf '[rust-fast] Invoke the script from the current checkout/worktree instead of reusing another checkout path.\n' >&2
  exit 1
fi

log() {
  printf '[rust-fast] %s\n' "$*"
}

usage() {
  cat <<'EOF'
Usage: scripts/test-rust-fast.sh <mode>

Modes:
  ipc           Run the IPC contract integration suites.
  layering      Run the Rust layering ratchet.
  lib-1         Run lib shard 1/2.
  lib-2         Run lib shard 2/2.
  lib           Run both lib shards sequentially against the shared target dir.
  lib-parallel  Run both lib shards in parallel with isolated target dirs.
  pr            Run the fast Rust CI subset: layering + IPC + lib shards.
  pr-parallel   Run the fast Rust CI subset with isolated per-lane target dirs.
  doc           Run doctests.
  full-integration
                Run the full Rust integration sweep.
  full-integration-archive <archive-file>
                Build the explicit full integration targets into a Nextest archive.
  main          Run the broad Rust CI aggregate: fast subset + doctests + full integration.
  help          Show this message.

Notes:
  - Sequential modes reuse the normal Cargo target dir and maximize cache reuse.
  - Parallel modes trade extra disk/compile work for lower wall-clock time by
    isolating CARGO_TARGET_DIR per lane.
  - All paths are checkout-local; in a worktree, caches live under that
    worktree's src-tauri/target and the script refuses to drift across checkouts.
  - Root lib and IPC lanes currently pass --features test-utils so Tauri mock
    tests stay covered while the backend test surface is being decoupled.
EOF
}

run_cmd() {
  local label="$1"
  shift
  log "${label}"
  (
    cd "${REPO_ROOT}"
    "$@"
  )
}

ipc_cmd() {
  cargo nextest run \
    --manifest-path "${MANIFEST_PATH}" \
    --profile ci \
    --features test-utils \
    --test suite_ipc_commands \
    ipc_contract
}

layering_cmd() {
  python3 scripts/check-layering.py
}

lib_shard_cmd() {
  local partition="$1"
  cargo nextest run \
    --manifest-path "${MANIFEST_PATH}" \
    --lib \
    --profile ci \
    --features test-utils \
    --partition "hash:${partition}"
}

doc_cmd() {
  cargo test --manifest-path "${MANIFEST_PATH}" --workspace --doc
}

full_integration_nextest_cmd() {
  local nextest_subcommand="$1"
  shift
  local test_args=()
  local test_name
  for test_name in "${FULL_INTEGRATION_TESTS[@]}"; do
    test_args+=(--test "${test_name}")
  done

  cargo nextest "${nextest_subcommand}" \
    --manifest-path "${MANIFEST_PATH}" \
    --profile ci \
    "${test_args[@]}" \
    --features test-utils \
    "$@"
}

full_integration_cmd() {
  full_integration_nextest_cmd run
}

full_integration_archive_cmd() {
  local archive_file="$1"
  full_integration_nextest_cmd archive --archive-file "${archive_file}"
}

run_ipc() {
  run_cmd "ipc" ipc_cmd
}

run_layering() {
  run_cmd "layering" layering_cmd
}

run_lib_shard() {
  local partition="$1"
  run_cmd "lib-${partition}" lib_shard_cmd "${partition}"
}

run_docs() {
  run_cmd "doc" doc_cmd
}

run_full_integration() {
  run_cmd "full-integration" full_integration_cmd
}

run_full_integration_archive() {
  local archive_file="$1"
  run_cmd "full-integration-archive" full_integration_archive_cmd "${archive_file}"
}

run_isolated_ipc() {
  local suffix="$1"
  (
    export CARGO_TARGET_DIR="${FAST_TARGET_ROOT}/${suffix}"
    log "ipc: CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
    cd "${REPO_ROOT}"
    ipc_cmd
  )
}

run_isolated_lib_shard() {
  local partition="$1"
  local suffix="$2"
  (
    export CARGO_TARGET_DIR="${FAST_TARGET_ROOT}/${suffix}"
    log "lib-${partition}: CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
    cd "${REPO_ROOT}"
    lib_shard_cmd "${partition}"
  )
}

wait_for_job() {
  local label="$1"
  local pid="$2"

  if wait "${pid}"; then
    log "${label}: ok"
    return 0
  fi

  local status=$?
  log "${label}: failed (${status})"
  return "${status}"
}

run_lib_parallel() {
  mkdir -p "${FAST_TARGET_ROOT}"

  run_isolated_lib_shard "1/2" "lib-1" &
  local pid_one=$!
  run_isolated_lib_shard "2/2" "lib-2" &
  local pid_two=$!

  local status=0
  wait_for_job "lib-1" "${pid_one}" || status=$?
  wait_for_job "lib-2" "${pid_two}" || status=$?
  return "${status}"
}

run_pr_parallel() {
  mkdir -p "${FAST_TARGET_ROOT}"

  run_isolated_ipc "ipc" &
  local pid_ipc=$!
  run_isolated_lib_shard "1/2" "lib-1" &
  local pid_one=$!
  run_isolated_lib_shard "2/2" "lib-2" &
  local pid_two=$!

  local status=0
  wait_for_job "ipc" "${pid_ipc}" || status=$?
  wait_for_job "lib-1" "${pid_one}" || status=$?
  wait_for_job "lib-2" "${pid_two}" || status=$?
  return "${status}"
}

mode="${1:-help}"

case "${mode}" in
  ipc)
    run_ipc
    ;;
  layering)
    run_layering
    ;;
  lib-1)
    run_lib_shard "1/2"
    ;;
  lib-2)
    run_lib_shard "2/2"
    ;;
  lib)
    run_lib_shard "1/2"
    run_lib_shard "2/2"
    ;;
  lib-parallel)
    run_lib_parallel
    ;;
  pr)
    run_layering
    run_ipc
    run_lib_shard "1/2"
    run_lib_shard "2/2"
    ;;
  pr-parallel)
    run_layering
    run_pr_parallel
    ;;
  doc)
    run_docs
    ;;
  full-integration)
    run_full_integration
    ;;
  full-integration-archive)
    archive_file="${2:-}"
    if [[ -z "${archive_file}" ]]; then
      printf '[rust-fast] full-integration-archive requires an output file.\n' >&2
      exit 1
    fi
    run_full_integration_archive "${archive_file}"
    ;;
  main)
    run_layering
    run_ipc
    run_lib_shard "1/2"
    run_lib_shard "2/2"
    run_docs
    run_full_integration
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac
