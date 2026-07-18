#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${ROOT_DIR}/.github/workflows/ci.yml"
RUST_RUNNER="${ROOT_DIR}/scripts/test-rust-fast.sh"
TEST_TMP="$(mktemp -d)"
REAL_CARGO="$(command -v cargo)"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

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
grep -Fq 'bash scripts/test-rust-fast.sh full-integration' <<< "${integration_job}" \
  || fail "Rust Full Integration does not use the canonical Rust test runner"

mkdir -p "${TEST_TMP}/bin"
cat >"${TEST_TMP}/bin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${CARGO_ARGS_LOG}"
EOF
chmod +x "${TEST_TMP}/bin/cargo"

CARGO_ARGS_LOG="${TEST_TMP}/cargo-args.log" \
  PATH="${TEST_TMP}/bin:${PATH}" \
  bash "${RUST_RUNNER}" full-integration >/dev/null

"${REAL_CARGO}" metadata \
  --manifest-path "${ROOT_DIR}/src-tauri/Cargo.toml" \
  --format-version 1 \
  --no-deps \
  | python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
root = next(package for package in metadata["packages"] if package["name"] == "ralphx")
print("\n".join(sorted(
    target["name"]
    for target in root["targets"]
    if "test" in target["kind"]
)))
' >"${TEST_TMP}/expected-targets.txt"

expected_targets=()
while IFS= read -r target; do
  expected_targets+=("${target}")
done <"${TEST_TMP}/expected-targets.txt"

[[ "${#expected_targets[@]}" -gt 0 ]] || fail "Cargo metadata returned no integration targets"
grep -Fxq -- '--manifest-path' "${TEST_TMP}/cargo-args.log" \
  || fail "Rust runner omitted the Cargo manifest"
grep -Fxq -- '--profile' "${TEST_TMP}/cargo-args.log" \
  || fail "Rust runner omitted the Nextest profile"
grep -Fxq -- '--features' "${TEST_TMP}/cargo-args.log" \
  || fail "Rust runner omitted test-utils features"

for target in "${expected_targets[@]}"; do
  awk -v target="${target}" '
    previous == "--test" && $0 == target { found = 1 }
    { previous = $0 }
    END { exit(found ? 0 : 1) }
  ' "${TEST_TMP}/cargo-args.log" \
    || fail "Rust full integration runner omits Cargo test target ${target}"
done

if grep -Fxq -- '--tests' "${TEST_TMP}/cargo-args.log" \
  || grep -Fxq -- '--lib' "${TEST_TMP}/cargo-args.log" \
  || grep -Fxq -- '--all-targets' "${TEST_TMP}/cargo-args.log"; then
  fail "Rust full integration runner also selects redundant unit-test targets"
fi

echo "PASS: Rust Full Integration selects every integration target without root-lib duplication"
