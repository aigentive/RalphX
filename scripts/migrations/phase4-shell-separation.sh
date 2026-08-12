#!/usr/bin/env bash
#
# Phase 4 — Shell Separation + Layering Un-Inversions
#
# Mechanical extraction script (CLAUDE.md rule 21). Every large move here is a
# `git mv` plus a scripted rewrite; hand edits are limited to the small
# post-move fix-up layer (visibility widening, import cleanup).
#
# Regenerate and re-run this off a fresh `main` before landing. Landing window
# is <= 1 day; on rebase, reset to HEAD and re-run rather than resolving
# conflicts by hand.
#
# Usage:
#   scripts/migrations/phase4-shell-separation.sh step1
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$REPO_ROOT/src-tauri/src"

# 13 composition-root modules that become the desktop shell.
MOVE_MODULES=(
  app_setup
  server_boot
  shutdown
  startup_pipeline
  startup_pipeline_launch
  startup_cleanup
  setup_settings
  startup_runtime_builders
  startup_transition_factory
  runtime_wiring
  native_menu
  dev_dock_icon
  startup_bootstrap
)

# 9 `*_tests.rs` sidecars belonging to the modules above.
MOVE_SIDECARS=(
  app_setup_tests
  server_boot_tests
  shutdown_tests
  startup_pipeline_tests
  startup_cleanup_tests
  setup_settings_tests
  startup_runtime_builders_tests
  runtime_wiring_tests
  startup_bootstrap_tests
)

# Pinned by non-moving application importers — moving these would invert
# application -> shell. See plan "Key Decisions".
#   startup_status              <- app_state.rs:25, startup_failure_classification.rs:7
#   startup_git_auth_preflight  <- app_state.rs:24
#   startup_background          <- automation/reopen.rs:7
DO_NOT_MOVE=(startup_status startup_background startup_git_auth_preflight startup_jobs startup_failure_classification)

MOVE_RE="$(IFS='|'; echo "${MOVE_MODULES[*]}")"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# Rewrite in place across the crate. Uses perl for portable -i without the
# BSD/GNU `sed -i` argument split.
rewrite() {
  local pattern="$1" replacement="$2"; shift 2
  perl -pi -e "s{$pattern}{$replacement}g" "$@"
}

crate_files() {
  find "$SRC" -name '*.rs' -type f
  find "$REPO_ROOT/src-tauri/tests" -name '*.rs' -type f 2>/dev/null || true
}

preflight() {
  log "Pre-flight assertions"
  for m in "${MOVE_MODULES[@]}"; do
    [ -f "$SRC/application/$m.rs" ] || die "missing move-set module: application/$m.rs"
  done
  for s in "${MOVE_SIDECARS[@]}"; do
    [ -f "$SRC/application/$s.rs" ] || die "missing move-set sidecar: application/$s.rs"
  done
  for m in "${DO_NOT_MOVE[@]}"; do
    [ -f "$SRC/application/$m.rs" ] || die "missing pinned module: application/$m.rs"
  done

  # Any NEW non-move-set application importer of a move-set module is a
  # stop-and-reclassify, not a shim opportunity: a shim would hide an
  # application -> shell inversion from the layering guard.
  local strays
  strays="$(rg -l -e "crate::application::($MOVE_RE)\b" "$SRC/application" 2>/dev/null \
    | grep -v -E "/($(IFS='|'; echo "${MOVE_MODULES[*]}")|$(IFS='|'; echo "${MOVE_SIDECARS[*]}"))\.rs$" \
    | grep -v '/http_shutdown\.rs$' || true)"
  [ -z "$strays" ] || die "unexpected application importers of move-set modules:\n$strays"

  log "Pre-flight OK (13 modules, 9 sidecars, 5 pinned)"
}

step1() {
  preflight

  log "Moving 22 files application/ -> shell/"
  # Depth is preserved (src/application -> src/shell), which keeps relative
  # asset anchors valid, notably dev_dock_icon.rs include_bytes!("../../icons/...").
  for f in "${MOVE_MODULES[@]}" "${MOVE_SIDECARS[@]}"; do
    git -C "$REPO_ROOT" mv "src-tauri/src/application/$f.rs" "src-tauri/src/shell/$f.rs"
  done

  log "Moving the Tauri invoke registry to the shell"
  # registry.rs is a single #[macro_export] macro_rules! with no imports; it
  # expands in lib.rs scope, so relocating the file changes nothing about path
  # resolution inside the macro body. It must live in the shell so that shell-
  # hosted commands can be registered without a commands -> shell edge.
  git -C "$REPO_ROOT" mv "src-tauri/src/commands/registry.rs" "src-tauri/src/shell/command_registry.rs"

  log "Rewriting crate::application::<moved> -> crate::shell::<moved>"
  # shellcheck disable=SC2046
  rewrite "crate::application::($MOVE_RE)\b" "crate::shell::\$1" $(crate_files)

  log "Fixing up intra-shell and shell -> application references"
  # server_boot_tests reached startup_status through `super::`, which now
  # resolves to `shell` instead of `application`.
  rewrite "use super::startup_status::" "use crate::application::startup_status::" \
    "$SRC/shell/server_boot_tests.rs"

  # startup_pipeline_launch called `application::startup_pipeline::...` via a
  # bare `use crate::application;`, which no longer resolves.
  rewrite "\bapplication::startup_pipeline::run_startup_pipeline" \
    "crate::shell::startup_pipeline::run_startup_pipeline" \
    "$SRC/shell/startup_pipeline_launch.rs"
  perl -ni -e 'print unless /^use crate::application;$/' "$SRC/shell/startup_pipeline_launch.rs"

  log "Rewriting lib.rs entrypoint references"
  rewrite "\bapplication::($MOVE_RE)::" "shell::\$1::" "$SRC/lib.rs"

  log "Step 1 mechanical pass done — module declarations and the two"
  log "commands -> shell symbol descents are applied as the fix-up layer."
}

step3() {
  log "Moving the agent spawner into the application layer"
  # The spawner drives lane resolution, execution slots and the harness runtime
  # registry, all of which live in `application`. Its only reason to sit in
  # `infrastructure` was history, and it was the last infrastructure -> commands
  # importer. Its `crate::infrastructure::*` uses stay valid and become ordinary
  # downward imports.
  mkdir -p "$SRC/application/agents"
  git -C "$REPO_ROOT" mv "src-tauri/src/infrastructure/agents/spawner.rs" \
    "src-tauri/src/application/agents/spawner.rs"
  git -C "$REPO_ROOT" mv "src-tauri/src/infrastructure/agents/spawner_tests.rs" \
    "src-tauri/src/application/agents/spawner_tests.rs"

  log "Repointing spawner paths"
  # shellcheck disable=SC2046
  rewrite "crate::infrastructure::agents::spawner::" "crate::application::agents::spawner::" $(crate_files)
  # shellcheck disable=SC2046
  rewrite "crate::infrastructure::agents::AgenticClientSpawner" \
    "crate::application::agents::AgenticClientSpawner" $(crate_files)
  # Integration suites reach the type through the crate's public path.
  # shellcheck disable=SC2046
  rewrite "ralphx_lib::infrastructure::agents::AgenticClientSpawner" \
    "ralphx_lib::application::agents::AgenticClientSpawner" $(crate_files)

  log "Step 3 mechanical pass done — module declarations are fix-up layer."
}

case "${1:-}" in
  step1) step1 ;;
  step3) step3 ;;
  preflight) preflight ;;
  *) die "usage: $(basename "$0") {preflight|step1|step3}" ;;
esac
