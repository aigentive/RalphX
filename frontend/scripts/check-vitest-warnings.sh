#!/usr/bin/env bash
# Regression guard: fail if the vitest runner emits Node's WebStorage
# "`--localstorage-file` was provided without a valid path" warning.
# Root cause: Node 24+ ships WebStorage on by default; suppressed via
# vitest.config.ts test.execArgv (--no-experimental-webstorage).
# Couples to a stable localStorage-touching test file as a fast probe.
set -euo pipefail
cd "$(dirname "$0")/.."
target="src/lib/postUpdatePreparing.test.ts"
output="$(./node_modules/.bin/vitest run "$target" 2>&1)"
if grep -qi -- "--localstorage-file" <<<"$output"; then
  echo "FAIL: vitest emitted the Node '--localstorage-file' WebStorage warning." >&2
  grep -i -- "--localstorage-file" <<<"$output" >&2
  exit 1
fi
echo "OK: no '--localstorage-file' warning from vitest."
