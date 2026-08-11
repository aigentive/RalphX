#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${ROOT_DIR}/.github/workflows/ci.yml"
RUST_RUNNER="${ROOT_DIR}/scripts/test-rust-fast.sh"
NEXTEST_CONFIG="${ROOT_DIR}/src-tauri/.config/nextest.toml"
# These values intentionally stay literal so the guard can match GitHub expressions.
# shellcheck disable=SC2016
MATRIX_SHARD_EXPR='${{ matrix.shard }}'
# shellcheck disable=SC2016
NEXTEST_VERSION_EXPR='${{ env.NEXTEST_VERSION }}'
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

integration_archive_job="$({
  awk '
    /^  rust-integration-nextest-archive:/ { capture = 1 }
    /^  rust-full-integration:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

integration_job="$({
  awk '
    /^  rust-full-integration:/ { capture = 1 }
    /^  rust-macos-build:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

lib_shard_job="$({
  awk '
    /^  rust-lib-tests:/ { capture = 1 }
    /^  rust-clippy:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

lib_archive_job="$({
  awk '
    /^  rust-lib-nextest-archive:/ { capture = 1 }
    /^  rust-lib-tests:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

ipc_job="$({
  awk '
    /^  rust-ipc-contracts:/ { capture = 1 }
    /^  rust-lib-nextest-archive:/ { capture = 0 }
    capture
  ' "${WORKFLOW}"
})"

[[ -n "${integration_archive_job}" ]] || fail "Rust integration archive job is missing"
[[ -n "${integration_job}" ]] || fail "Rust Full Integration job is missing"
[[ -n "${lib_shard_job}" ]] || fail "Rust lib shard job is missing"
[[ -n "${lib_archive_job}" ]] || fail "Rust lib archive job is missing"
[[ -n "${ipc_job}" ]] || fail "Rust IPC contract job is missing"
grep -Fq 'bash scripts/test-rust-fast.sh full-integration-archive rust-integration-tests.tar.zst' <<< "${integration_archive_job}" \
  || fail "Rust integration archive does not use the canonical explicit-target runner"
grep -Fq 'name: rust-integration-tests-nextest-archive' <<< "${integration_archive_job}" \
  || fail "Rust integration archive is not uploaded for shard reuse"
grep -Fq 'shard: [1, 2]' <<< "${integration_job}" \
  || fail "Rust Full Integration does not define two execution shards"
grep -Fq -- '--archive-file rust-integration-tests.tar.zst' <<< "${integration_job}" \
  || fail "Rust Full Integration does not execute the shared archive"
grep -Fq -- "--partition slice:${MATRIX_SHARD_EXPR}/2" <<< "${integration_job}" \
  || fail "Rust Full Integration does not use balanced slice partitioning"

if grep -Fq 'CARGO_BUILD_JOBS' <<< "${integration_archive_job}${integration_job}"; then
  fail "Rust integration compilation is still artificially serialized"
fi

grep -Fq 'shard: [1, 2]' <<< "${lib_shard_job}" \
  || fail "Rust lib tests do not define two execution shards"
grep -Fq -- "--partition hash:${MATRIX_SHARD_EXPR}/2" <<< "${lib_shard_job}" \
  || fail "Rust lib tests do not execute two deterministic archive partitions"

grep -Fq 'shared-key: rust-test-deps' <<< "${lib_archive_job}" \
  || fail "Rust lib archive does not restore the shared test dependency cache"
grep -Fq 'save-if: "false"' <<< "${lib_archive_job}" \
  || fail "Rust lib archive is not an explicit restore-only cache consumer"
grep -Fq 'shared-key: rust-test-deps' <<< "${integration_archive_job}" \
  || fail "Rust integration archive does not restore the shared test dependency cache"
grep -Fq 'cache-on-failure: true' <<< "${integration_archive_job}" \
  || fail "Rust integration archive does not preserve dependencies after a failed build"
grep -Fq 'save-if: "true"' <<< "${integration_archive_job}" \
  || fail "Rust integration archive is not the designated dependency cache writer"

for compile_job in "${ipc_job}" "${lib_archive_job}" "${integration_archive_job}"; do
  grep -Fq 'CARGO_PROFILE_TEST_DEBUG: "1"' <<< "${compile_job}" \
    || fail "A shared Rust test-cache compile job does not use reduced CI debuginfo"
done

grep -Fq 'NEXTEST_VERSION: "0.9.133"' "${WORKFLOW}" \
  || fail "Rust CI does not centrally pin the known-good cargo-nextest version"
nextest_install_count="$(grep -Fc 'uses: taiki-e/install-action@' "${WORKFLOW}")"
nextest_pin_count="$(grep -Fc "tool: nextest@${NEXTEST_VERSION_EXPR}" "${WORKFLOW}")"
[[ "${nextest_install_count}" -eq "${nextest_pin_count}" ]] \
  || fail "Not every cargo-nextest install uses the central version pin"

grep -Fq '[profile.ci.junit]' "${NEXTEST_CONFIG}" \
  || fail "The CI nextest profile does not emit JUnit timing data"
grep -Fq 'path = "junit.xml"' "${NEXTEST_CONFIG}" \
  || fail "The CI nextest profile does not use the canonical JUnit output path"
grep -Fq "name: rust-lib-tests-junit-${MATRIX_SHARD_EXPR}-of-2" <<< "${lib_shard_job}" \
  || fail "Rust lib shards do not publish uniquely named JUnit artifacts"
grep -Fq "name: rust-integration-tests-junit-${MATRIX_SHARD_EXPR}-of-2" <<< "${integration_job}" \
  || fail "Rust integration shards do not publish uniquely named JUnit artifacts"
for shard_job in "${lib_shard_job}" "${integration_job}"; do
  grep -Fq 'if: always()' <<< "${shard_job}" \
    || fail "A Rust shard drops its JUnit report after test failure"
  grep -Fq 'path: src-tauri/target/nextest/ci/junit.xml' <<< "${shard_job}" \
    || fail "A Rust shard uploads JUnit data from the wrong path"
done

mkdir -p "${TEST_TMP}/bin"
cat >"${TEST_TMP}/bin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${CARGO_ARGS_LOG}"
EOF
chmod +x "${TEST_TMP}/bin/cargo"

CARGO_ARGS_LOG="${TEST_TMP}/cargo-args.log" \
  PATH="${TEST_TMP}/bin:${PATH}" \
  bash "${RUST_RUNNER}" full-integration >/dev/null

CARGO_ARGS_LOG="${TEST_TMP}/cargo-archive-args.log" \
  PATH="${TEST_TMP}/bin:${PATH}" \
  bash "${RUST_RUNNER}" full-integration-archive rust-integration-tests.tar.zst >/dev/null

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

grep -Fxq -- 'archive' "${TEST_TMP}/cargo-archive-args.log" \
  || fail "Rust integration archive mode does not invoke nextest archive"
grep -Fxq -- '--archive-file' "${TEST_TMP}/cargo-archive-args.log" \
  || fail "Rust integration archive mode omits its output file"

for target in "${expected_targets[@]}"; do
  awk -v target="${target}" '
    previous == "--test" && $0 == target { found = 1 }
    { previous = $0 }
    END { exit(found ? 0 : 1) }
  ' "${TEST_TMP}/cargo-archive-args.log" \
    || fail "Rust integration archive omits Cargo test target ${target}"
done

echo "PASS: Rust Full Integration selects every integration target without root-lib duplication"
