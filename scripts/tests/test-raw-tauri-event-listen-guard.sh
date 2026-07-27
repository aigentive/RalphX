#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE_ROOT="$(mktemp -d "${ROOT_DIR}/.artifacts/raw-listen-guard.XXXXXX")"
trap 'rm -rf "${FIXTURE_ROOT}"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

mkdir -p "${FIXTURE_ROOT}/frontend/src/lib" "${FIXTURE_ROOT}/frontend/src/components"
printf '%s\n' 'import { listen } from "@tauri-apps/api/event";' \
  >"${FIXTURE_ROOT}/frontend/src/lib/event-bus.ts"

node "${ROOT_DIR}/scripts/check-raw-tauri-event-listen.mjs" "${FIXTURE_ROOT}" \
  >/dev/null || fail "guard rejected its explicit event-bus allowlist"

printf '%s\n' 'import { listen } from "@tauri-apps/api/event";' \
  'void listen("drift", () => undefined);' \
  >"${FIXTURE_ROOT}/frontend/src/components/RawListener.tsx"

if node "${ROOT_DIR}/scripts/check-raw-tauri-event-listen.mjs" "${FIXTURE_ROOT}" \
  >"${FIXTURE_ROOT}/guard.out" 2>&1; then
  fail "guard accepted a raw-listen reintroduction"
fi
grep -Fq "frontend/src/components/RawListener.tsx:1" "${FIXTURE_ROOT}/guard.out" \
  || fail "guard failure did not identify the raw-listen fixture"

node "${ROOT_DIR}/scripts/check-raw-tauri-event-listen.mjs" "${ROOT_DIR}" \
  >/dev/null || fail "guard rejected the current repository tree"

echo "PASS: raw Tauri listen guard rejects drift and accepts the current tree"
