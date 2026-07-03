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
  lib-1         Run lib shard 1/2.
  lib-2         Run lib shard 2/2.
  lib           Run both lib shards sequentially against the shared target dir.
  lib-parallel  Run both lib shards in parallel with isolated target dirs.
  pr            Reproduce PR Rust CI locally: IPC + lib shards sequentially.
  pr-parallel   Reproduce PR Rust CI locally with isolated per-lane target dirs.
  doc           Run doctests.
  main          Reproduce push-to-main Rust CI locally: PR stack + doctests.
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
    --test task_commands \
    --test api_key_commands \
    --test project_commands \
    --test harness_provider_commands \
    --test unified_chat_commands \
    --test task_step_commands \
    ipc_contract
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
  cargo test --manifest-path "${MANIFEST_PATH}" --doc
}

run_ipc() {
  run_cmd "ipc" ipc_cmd
}

run_lib_shard() {
  local partition="$1"
  run_cmd "lib-${partition}" lib_shard_cmd "${partition}"
}

run_docs() {
  run_cmd "doc" doc_cmd
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
    run_ipc
    run_lib_shard "1/2"
    run_lib_shard "2/2"
    ;;
  pr-parallel)
    run_pr_parallel
    ;;
  doc)
    run_docs
    ;;
  main)
    run_ipc
    run_lib_shard "1/2"
    run_lib_shard "2/2"
    run_docs
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac
