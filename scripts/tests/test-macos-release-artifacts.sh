#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VALIDATOR="${ROOT_DIR}/scripts/validate-macos-release-artifacts.sh"
BUILD_SCRIPT="${ROOT_DIR}/scripts/build-prod-release.sh"
WORKFLOW_RUNNER_SCRIPT="${ROOT_DIR}/src-tauri/scripts/build-workflow-runner.sh"
TAURI_CONFIG="${ROOT_DIR}/src-tauri/tauri.conf.json"
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "${TEST_TMP}"' EXIT

pass_count=0

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

pass() {
  pass_count=$((pass_count + 1))
  echo "PASS: $*"
}

assert_fails() {
  local output_file="$1"
  shift
  if "$@" >"${output_file}" 2>&1; then
    fail "command unexpectedly succeeded: $*"
  fi
}

assert_log_order() {
  local log_file="$1"
  shift
  local previous_line=0
  local pattern line
  for pattern in "$@"; do
    line="$(grep -n -m 1 -F -- "${pattern}" "${log_file}" | cut -d: -f1 || true)"
    [[ -n "${line}" ]] || fail "missing log entry: ${pattern}"
    (( line > previous_line )) || fail "out-of-order log entry: ${pattern}"
    previous_line="${line}"
  done
}

make_artifacts() {
  local root="$1"
  mkdir -p "${root}/RalphX.app/Contents/MacOS"
  : > "${root}/RalphX.app/Contents/Info.plist"
  : > "${root}/RalphX.app/Contents/MacOS/ralphx"
  chmod +x "${root}/RalphX.app/Contents/MacOS/ralphx"
  : > "${root}/RalphX.dmg"
}

make_stub_tools() {
  local bin_dir="$1"
  mkdir -p "${bin_dir}"

  cat >"${bin_dir}/codesign" <<'EOF'
#!/usr/bin/env bash
echo "codesign $*" >>"${STUB_LOG}"
[[ "${STUB_CODESIGN_FAIL:-}" != "true" ]]
EOF

  cat >"${bin_dir}/xcrun" <<'EOF'
#!/usr/bin/env bash
echo "xcrun $*" >>"${STUB_LOG}"
if [[ "${1:-}" == "stapler" && "${2:-}" == "validate" && "${STUB_STAPLER_VALIDATE_FAIL_FOR:-}" == "${3:-}" ]]; then
  exit 1
fi
if [[ "${1:-}" == "stapler" && "${2:-}" == "staple" && "${STUB_STAPLER_STAPLE_FAIL:-}" == "true" ]]; then
  exit 1
fi
if [[ "${1:-}" == "notarytool" && "${STUB_NOTARY_FAIL:-}" == "true" ]]; then
  exit 1
fi
exit 0
EOF

  cat >"${bin_dir}/spctl" <<'EOF'
#!/usr/bin/env bash
echo "spctl $*" >>"${STUB_LOG}"
if [[ "${STUB_SPCTL_FAIL_TYPE:-}" == "execute" && "$*" == *"--type execute"* ]]; then
  exit 1
fi
if [[ "${STUB_SPCTL_FAIL_TYPE:-}" == "open" && "$*" == *"--type open"* ]]; then
  exit 1
fi
exit 0
EOF

  cat >"${bin_dir}/syspolicy_check" <<'EOF'
#!/usr/bin/env bash
echo "syspolicy_check $*" >>"${STUB_LOG}"
[[ "${STUB_SYSPOLICY_FAIL:-}" != "true" ]]
EOF

  cat >"${bin_dir}/plutil" <<'EOF'
#!/usr/bin/env bash
echo "plutil $*" >>"${STUB_LOG}"
case "${2:-}" in
  CFBundleIdentifier) echo "com.ralphx.app" ;;
  CFBundleExecutable) echo "ralphx" ;;
  LSMinimumSystemVersion) echo "13.0" ;;
  LSRequiresCarbon) echo "true" ;;
  *) exit 1 ;;
esac
EOF

  cat >"${bin_dir}/lipo" <<'EOF'
#!/usr/bin/env bash
echo "lipo $*" >>"${STUB_LOG}"
echo "${STUB_LIPO_ARCH:-arm64}"
EOF

  cat >"${bin_dir}/npm" <<'EOF'
#!/usr/bin/env bash
echo "npm $*" >>"${STUB_LOG}"
exit 0
EOF

  chmod +x "${bin_dir}"/*
}

run_validator() {
  local fixture="$1"
  local expected_arch="${2:-aarch64}"
  "${VALIDATOR}" \
    --app "${fixture}/RalphX.app" \
    --dmg "${fixture}/RalphX.dmg" \
    --expected-arch "${expected_arch}"
}

test_validator_happy_path() {
  local case_dir="${TEST_TMP}/validator-happy"
  local bin_dir="${case_dir}/bin"
  local artifact_dir="${case_dir}/artifacts"
  local log_file="${case_dir}/commands.log"
  make_stub_tools "${bin_dir}"
  make_artifacts "${artifact_dir}"

  PATH="${bin_dir}:/usr/bin:/bin" STUB_LOG="${log_file}" run_validator "${artifact_dir}"

  assert_log_order "${log_file}" \
    "codesign --verify --deep --strict" \
    "xcrun stapler validate ${artifact_dir}/RalphX.app" \
    "spctl --assess --type execute" \
    "syspolicy_check distribution ${artifact_dir}/RalphX.app" \
    "lipo -archs ${artifact_dir}/RalphX.app/Contents/MacOS/ralphx" \
    "codesign --verify --strict" \
    "xcrun stapler validate ${artifact_dir}/RalphX.dmg" \
    "spctl --assess --type open --context context:primary-signature"
  pass "validator accepts a fully trusted per-architecture release"
}

test_validator_rejects_unstapled_dmg() {
  local case_dir="${TEST_TMP}/validator-unstapled"
  local bin_dir="${case_dir}/bin"
  local artifact_dir="${case_dir}/artifacts"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_stub_tools "${bin_dir}"
  make_artifacts "${artifact_dir}"

  assert_fails "${output_file}" env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    STUB_STAPLER_VALIDATE_FAIL_FOR="${artifact_dir}/RalphX.dmg" \
    "${VALIDATOR}" --app "${artifact_dir}/RalphX.app" --dmg "${artifact_dir}/RalphX.dmg" --expected-arch aarch64

  grep -qi "DMG.*stapl\|stapl.*DMG" "${output_file}" || fail "unstapled DMG diagnostic was not explicit"
  pass "validator rejects a v0.67.0-style unstapled DMG"
}

test_validator_rejects_wrong_architecture() {
  local case_dir="${TEST_TMP}/validator-arch"
  local bin_dir="${case_dir}/bin"
  local artifact_dir="${case_dir}/artifacts"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_stub_tools "${bin_dir}"
  make_artifacts "${artifact_dir}"

  assert_fails "${output_file}" env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    STUB_LIPO_ARCH="x86_64" \
    "${VALIDATOR}" --app "${artifact_dir}/RalphX.app" --dmg "${artifact_dir}/RalphX.dmg" --expected-arch aarch64

  grep -q "architecture" "${output_file}" || fail "architecture mismatch diagnostic was not explicit"
  pass "validator rejects the wrong release architecture"
}

test_validator_rejects_dmg_gatekeeper_failure() {
  local case_dir="${TEST_TMP}/validator-gatekeeper"
  local bin_dir="${case_dir}/bin"
  local artifact_dir="${case_dir}/artifacts"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_stub_tools "${bin_dir}"
  make_artifacts "${artifact_dir}"

  assert_fails "${output_file}" env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    STUB_SPCTL_FAIL_TYPE="open" \
    "${VALIDATOR}" --app "${artifact_dir}/RalphX.app" --dmg "${artifact_dir}/RalphX.dmg" --expected-arch aarch64

  grep -qi "DMG.*Gatekeeper\|Gatekeeper.*DMG" "${output_file}" || fail "DMG Gatekeeper diagnostic was not explicit"
  pass "validator rejects a DMG that Gatekeeper refuses"
}

make_build_fixture() {
  local project_dir="$1"
  local bin_dir="$2"
  mkdir -p "${project_dir}/scripts" "${project_dir}/frontend"
  cp "${BUILD_SCRIPT}" "${project_dir}/scripts/build-prod-release.sh"
  cp "${VALIDATOR}" "${project_dir}/scripts/validate-macos-release-artifacts.sh"
  chmod +x "${project_dir}/scripts/"*.sh
  cat >"${project_dir}/scripts/prepare-release-runtime.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "prepare-release-runtime" >>"${STUB_LOG}"
EOF
  chmod +x "${project_dir}/scripts/prepare-release-runtime.sh"
  make_stub_tools "${bin_dir}"
}

make_build_artifacts() {
  local project_dir="$1"
  local release_dir="${project_dir}/src-tauri/target/aarch64-apple-darwin/release"
  make_artifacts "${release_dir}/bundle/macos"
  mkdir -p "${release_dir}/bundle/dmg"
  mv "${release_dir}/bundle/macos/RalphX.dmg" "${release_dir}/bundle/dmg/RalphX.dmg"
  : > "${release_dir}/bundle/macos/RalphX.app.tar.gz"
  : > "${release_dir}/bundle/macos/RalphX.app.tar.gz.sig"
  : > "${release_dir}/ralphx"
  chmod +x "${release_dir}/ralphx"
}

run_build_fixture() {
  local project_dir="$1"
  local bin_dir="$2"
  local log_file="$3"
  env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    APPLE_API_KEY="KEY-ID" \
    APPLE_API_ISSUER="ISSUER-ID" \
    APPLE_API_KEY_PATH="${project_dir}/private-key.p8" \
    "${project_dir}/scripts/build-prod-release.sh" \
      --target aarch64-apple-darwin \
      --expected-arch aarch64
}

test_build_notarizes_before_validation() {
  local case_dir="${TEST_TMP}/build-order"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"
  : > "${project_dir}/private-key.p8"

  run_build_fixture "${project_dir}" "${bin_dir}" "${log_file}"

  assert_log_order "${log_file}" \
    "xcrun notarytool submit" \
    "xcrun stapler staple" \
    "codesign --verify --deep --strict"
  pass "production build submits and staples the final DMG before validation"
}

test_build_rejects_ambiguous_dmgs() {
  local case_dir="${TEST_TMP}/build-ambiguous"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"
  : > "${project_dir}/src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/duplicate.dmg"
  : > "${project_dir}/private-key.p8"

  assert_fails "${output_file}" run_build_fixture "${project_dir}" "${bin_dir}" "${log_file}"
  grep -qi "exactly one DMG" "${output_file}" || fail "ambiguous DMG diagnostic was not explicit"
  if [[ -f "${log_file}" ]] && grep -q "notarytool submit" "${log_file}"; then
    fail "ambiguous artifact selection reached Apple submission"
  fi
  pass "production build rejects ambiguous DMG artifacts before submission"
}

test_build_stops_on_notarization_rejection() {
  local case_dir="${TEST_TMP}/build-notary-rejection"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"
  : > "${project_dir}/private-key.p8"

  assert_fails "${output_file}" env \
    STUB_NOTARY_FAIL="true" \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    APPLE_API_KEY="KEY-ID" \
    APPLE_API_ISSUER="ISSUER-ID" \
    APPLE_API_KEY_PATH="${project_dir}/private-key.p8" \
    "${project_dir}/scripts/build-prod-release.sh" \
      --target aarch64-apple-darwin \
      --expected-arch aarch64

  grep -q "final-dmg-notarization-failed" "${output_file}" || fail "notarization rejection stage was not recorded"
  if grep -q "stapler staple\|codesign --verify" "${log_file}"; then
    fail "notarization rejection continued to stapling or validation"
  fi
  pass "production build stops on final-DMG notarization rejection"
}

test_build_stops_on_dmg_stapling_failure() {
  local case_dir="${TEST_TMP}/build-stapling-failure"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"
  : > "${project_dir}/private-key.p8"

  assert_fails "${output_file}" env \
    STUB_STAPLER_STAPLE_FAIL="true" \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    APPLE_API_KEY="KEY-ID" \
    APPLE_API_ISSUER="ISSUER-ID" \
    APPLE_API_KEY_PATH="${project_dir}/private-key.p8" \
    "${project_dir}/scripts/build-prod-release.sh" \
      --target aarch64-apple-darwin \
      --expected-arch aarch64

  grep -q "final-dmg-stapling-failed" "${output_file}" || fail "DMG stapling failure stage was not recorded"
  if grep -q "codesign --verify" "${log_file}"; then
    fail "DMG stapling failure continued to artifact validation"
  fi
  pass "production build stops when final-DMG stapling fails"
}

test_workflow_heartbeat_omits_process_arguments() {
  local workflow="${ROOT_DIR}/.github/workflows/release.yml"
  if grep -Fq 'ps -axo pid,ppid,etime,stat,command' "${workflow}"; then
    fail "release heartbeat persists full process arguments into uploaded trace artifacts"
  fi
  grep -Fq 'ps -axo pid,ppid,etime,stat,comm' "${workflow}" || fail "release heartbeat does not expose sanitized process telemetry"
  pass "release heartbeat omits secret-bearing process arguments"
}

test_tauri_build_hook_builds_requested_workflow_runner_target() {
  local case_dir="${TEST_TMP}/workflow-runner-hook"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_dir="${case_dir}/logs"
  local target_dir="${project_dir}/src-tauri/target"
  local hook

  mkdir -p \
    "${project_dir}/frontend" \
    "${project_dir}/src-tauri/scripts" \
    "${bin_dir}" \
    "${log_dir}"
  cp "${TAURI_CONFIG}" "${project_dir}/src-tauri/tauri.conf.json"
  cp "${WORKFLOW_RUNNER_SCRIPT}" "${project_dir}/src-tauri/scripts/build-workflow-runner.sh"
  chmod +x "${project_dir}/src-tauri/scripts/build-workflow-runner.sh"

  cat >"${bin_dir}/rustc" <<'EOF'
#!/usr/bin/env bash
cat <<'VERSION'
rustc 1.91.0 (fake)
binary: rustc
host: aarch64-apple-darwin
release: 1.91.0
VERSION
EOF

  cat >"${bin_dir}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "${PWD}" >"${STUB_LOG_DIR}/cargo-cwd.log"
printf '%s\n' "$*" >"${STUB_LOG_DIR}/cargo-args.log"

profile="debug"
target=""
previous=""
for argument in "$@"; do
  if [[ "${previous}" == "--target" ]]; then
    target="${argument}"
  fi
  if [[ "${argument}" == "--release" ]]; then
    profile="release"
  fi
  previous="${argument}"
done

[[ -n "${target}" ]] || { echo "cargo stub requires --target" >&2; exit 1; }
mkdir -p "${CARGO_TARGET_DIR}/${target}/${profile}"
: >"${CARGO_TARGET_DIR}/${target}/${profile}/ralphx-workflow-runner"
chmod +x "${CARGO_TARGET_DIR}/${target}/${profile}/ralphx-workflow-runner"
EOF

  cat >"${bin_dir}/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "${PWD}" >"${STUB_LOG_DIR}/npm-cwd.log"
printf '%s\n' "$*" >"${STUB_LOG_DIR}/npm-args.log"
EOF
  chmod +x "${bin_dir}/rustc" "${bin_dir}/cargo" "${bin_dir}/npm"

  hook="$(jq -r '.build.beforeBuildCommand | if type == "object" then .script else . end' "${project_dir}/src-tauri/tauri.conf.json")"
  (
    cd "${project_dir}/frontend"
    env \
      PATH="${bin_dir}:/usr/bin:/bin" \
      STUB_LOG_DIR="${log_dir}" \
      CARGO_TARGET_DIR="${target_dir}" \
      TAURI_ENV_TARGET_TRIPLE="x86_64-apple-darwin" \
      sh -c "${hook}"
  )

  grep -Fqx "${project_dir}/src-tauri" "${log_dir}/cargo-cwd.log" || fail "workflow runner Cargo build did not anchor to src-tauri"
  grep -Fqx "build -p ralphx-workflow-runner --release --target x86_64-apple-darwin" "${log_dir}/cargo-args.log" || fail "workflow runner Cargo build ignored the requested release target"
  grep -Fqx "${project_dir}/frontend" "${log_dir}/npm-cwd.log" || fail "Tauri frontend build did not run from the frontend directory"
  grep -Fq "run build" "${log_dir}/npm-args.log" || fail "Tauri hook did not run the frontend build"
  [[ -x "${project_dir}/src-tauri/binaries/ralphx-workflow-runner-x86_64-apple-darwin" ]] || fail "requested workflow runner sidecar was not copied into Tauri binaries"
  [[ ! -e "${project_dir}/src-tauri/binaries/ralphx-workflow-runner-aarch64-apple-darwin" ]] || fail "cross-target hook incorrectly packaged the ARM host binary"
  pass "Tauri build hook builds and packages the requested workflow runner target"
}

test_skip_build_validates_without_submission() {
  local case_dir="${TEST_TMP}/skip-build"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"

  env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    "${project_dir}/scripts/build-prod-release.sh" \
      --skip-build \
      --target aarch64-apple-darwin \
      --expected-arch aarch64

  if grep -q "notarytool submit\|stapler staple" "${log_file}"; then
    fail "--skip-build performed a notarization mutation"
  fi
  grep -q "codesign --verify --deep --strict" "${log_file}" || fail "--skip-build did not validate existing artifacts"
  pass "--skip-build remains validation-only"
}

test_build_requires_notarization_credentials_without_leaking_values() {
  local case_dir="${TEST_TMP}/build-credentials"
  local project_dir="${case_dir}/project"
  local bin_dir="${case_dir}/bin"
  local log_file="${case_dir}/commands.log"
  local output_file="${case_dir}/output.log"
  make_build_fixture "${project_dir}" "${bin_dir}"
  make_build_artifacts "${project_dir}"

  assert_fails "${output_file}" env \
    PATH="${bin_dir}:/usr/bin:/bin" \
    STUB_LOG="${log_file}" \
    APPLE_API_KEY="SECRET-KEY-ID" \
    APPLE_API_ISSUER="" \
    APPLE_API_KEY_PATH="${project_dir}/missing-secret.p8" \
    "${project_dir}/scripts/build-prod-release.sh" \
      --target aarch64-apple-darwin \
      --expected-arch aarch64

  grep -q "APPLE_API_ISSUER" "${output_file}" || fail "missing credential diagnostic was not explicit"
  if grep -q "SECRET-KEY-ID" "${output_file}"; then
    fail "credential value leaked into failure output"
  fi
  pass "production build fails closed on missing notarization credentials"
}

test_validator_happy_path
test_validator_rejects_unstapled_dmg
test_validator_rejects_wrong_architecture
test_validator_rejects_dmg_gatekeeper_failure
test_build_notarizes_before_validation
test_build_rejects_ambiguous_dmgs
test_build_stops_on_notarization_rejection
test_build_stops_on_dmg_stapling_failure
test_skip_build_validates_without_submission
test_build_requires_notarization_credentials_without_leaking_values
test_workflow_heartbeat_omits_process_arguments
test_tauri_build_hook_builds_requested_workflow_runner_target

echo "All ${pass_count} macOS release artifact tests passed."
