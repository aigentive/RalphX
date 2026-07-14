#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Build RalphX production release artifacts without mutating local app data.

Usage:
  ./scripts/build-prod-release.sh [--clean] [--skip-build] [--target <triple>] [--expected-arch <arch>]

Options:
  --clean       Remove existing release bundle artifacts before building
  --skip-build  Skip the build step and only validate/report artifact paths
  --target      Build against a Rust/Tauri target triple, e.g. x86_64-apple-darwin
  --expected-arch  Validate aarch64 or x86_64 release contents
  -h, --help    Show this help

This script is production-oriented:
- it never seeds Application Support from the dev DB
- it never copies plugin runtime into Application Support
- it is the correct base entrypoint for CI/release automation

Use ./scripts/build-local-release.sh for internal local release-like workflows.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PREPARE_RUNTIME_SCRIPT="${PROJECT_ROOT}/scripts/prepare-release-runtime.sh"
VALIDATE_ARTIFACTS_SCRIPT="${PROJECT_ROOT}/scripts/validate-macos-release-artifacts.sh"
TRACE_DIR="${RALPHX_RELEASE_TRACE_DIR:-${PROJECT_ROOT}/.artifacts/release-trace}"
RAW_TRACE_LOG="${TRACE_DIR}/tauri-build.log"
STAGE_TRACE_LOG="${TRACE_DIR}/release-stages.log"

CLEAN="${RALPHX_RELEASE_CLEAN_BUNDLE:-false}"
SKIP_BUILD="false"
BUILD_TARGET="${RALPHX_RELEASE_TARGET:-}"
EXPECTED_ARCH="${RALPHX_RELEASE_ARCH:-}"

timestamp_utc() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

emit_stage_marker() {
  local message="$1"
  local timestamp
  timestamp="$(timestamp_utc)"
  printf '[release-stage] %s %s\n' "${timestamp}" "${message}" | tee -a "${STAGE_TRACE_LOG}"
}

prepare_trace_dir() {
  mkdir -p "${TRACE_DIR}"
  : > "${RAW_TRACE_LOG}"
  : > "${STAGE_TRACE_LOG}"
}

handle_release_error() {
  local exit_code=$?
  trap - ERR
  emit_stage_marker "release-script-failed exit_code=${exit_code}"
  exit "${exit_code}"
}

stream_tauri_build_output() {
  local line
  local saw_binary_built=0
  local saw_bundling=0
  local saw_identity=0
  local saw_signing=0
  local saw_notarize=0
  local saw_staple=0
  local saw_bundle_output=0

  while IFS= read -r line; do
    printf '%s\n' "${line}"

    if [[ "${saw_binary_built}" -eq 0 && "${line}" == *"Built application at:"* ]]; then
      emit_stage_marker "rust-binary-built"
      saw_binary_built=1
    fi

    if [[ "${saw_bundling}" -eq 0 && "${line}" == *"Bundling "* ]]; then
      emit_stage_marker "bundle-generation-started"
      saw_bundling=1
    fi

    if [[ "${saw_identity}" -eq 0 && "${line}" == *"found cert "* ]]; then
      emit_stage_marker "signing-identity-selected"
      saw_identity=1
    fi

    if [[ "${saw_signing}" -eq 0 && "${line}" == *"Signing with identity "* ]]; then
      emit_stage_marker "codesign-started"
      saw_signing=1
    fi

    if [[ "${saw_notarize}" -eq 0 ]]; then
      case "${line}" in
        *"notar"*|*"Notar"*)
          emit_stage_marker "notarization-activity"
          saw_notarize=1
          ;;
      esac
    fi

    if [[ "${saw_staple}" -eq 0 ]]; then
      case "${line}" in
        *"staple"*|*"Staple"*|*"stapling"*|*"Stapling"*)
          emit_stage_marker "stapling-activity"
          saw_staple=1
          ;;
      esac
    fi

    if [[ "${saw_bundle_output}" -eq 0 ]]; then
      case "${line}" in
        *".app.tar.gz"*|*"Finished 1 bundle at:"*|*"Finished 2 bundles at:"*|*"Finished bundle at:"*)
          emit_stage_marker "bundle-output-generated"
          saw_bundle_output=1
          ;;
      esac
    fi
  done
}

run_tauri_release_build() {
  local tauri_status=0
  local tauri_args=(build -- --verbose)

  if [[ -n "${BUILD_TARGET}" ]]; then
    tauri_args+=(--target "${BUILD_TARGET}")
  fi

  emit_stage_marker "frontend-tauri-build-started"

  (
    cd "${PROJECT_ROOT}/frontend"
    CI=false npm run tauri "${tauri_args[@]}"
  ) > >(tee "${RAW_TRACE_LOG}" | stream_tauri_build_output) 2>&1 || tauri_status=$?

  if [[ "${tauri_status}" -ne 0 ]]; then
    emit_stage_marker "frontend-tauri-build-failed exit_code=${tauri_status}"
    return "${tauri_status}"
  fi

  emit_stage_marker "frontend-tauri-build-completed"
}

infer_expected_arch() {
  if [[ -n "${EXPECTED_ARCH}" ]]; then
    return
  fi

  case "${BUILD_TARGET}" in
    aarch64-apple-darwin) EXPECTED_ARCH="aarch64" ;;
    x86_64-apple-darwin) EXPECTED_ARCH="x86_64" ;;
    "")
      case "$(uname -m)" in
        arm64) EXPECTED_ARCH="aarch64" ;;
        x86_64) EXPECTED_ARCH="x86_64" ;;
        *) echo "Unable to infer release architecture; pass --expected-arch" >&2; exit 1 ;;
      esac
      ;;
    *) echo "Unable to infer release architecture from target ${BUILD_TARGET}; pass --expected-arch" >&2; exit 1 ;;
  esac
}

resolve_single_dmg() {
  local matches=()
  local path
  if [[ -d "${DMG_DIR}" ]]; then
    while IFS= read -r -d '' path; do
      matches+=("${path}")
    done < <(find "${DMG_DIR}" -maxdepth 1 -type f -name '*.dmg' -print0)
  fi

  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "Expected exactly one DMG under ${DMG_DIR}; found ${#matches[@]}" >&2
    return 1
  fi

  printf '%s\n' "${matches[0]}"
}

require_notarization_credentials() {
  local missing=0
  local name
  for name in APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH; do
    if [[ -z "${!name:-}" ]]; then
      echo "Missing required notarization credential: ${name}" >&2
      missing=1
    fi
  done
  if [[ -n "${APPLE_API_KEY_PATH:-}" && ! -f "${APPLE_API_KEY_PATH}" ]]; then
    echo "Notarization API key file not found at APPLE_API_KEY_PATH" >&2
    missing=1
  fi
  [[ "${missing}" -eq 0 ]]
}

notarize_final_dmg() {
  local dmg_path="$1"
  require_notarization_credentials

  emit_stage_marker "final-dmg-notarization-submission-started"
  if ! xcrun notarytool submit "${dmg_path}" \
    --key "${APPLE_API_KEY_PATH}" \
    --key-id "${APPLE_API_KEY}" \
    --issuer "${APPLE_API_ISSUER}" \
    --wait; then
    emit_stage_marker "final-dmg-notarization-failed"
    return 1
  fi
  emit_stage_marker "final-dmg-notarization-accepted"

  emit_stage_marker "final-dmg-stapling-started"
  if ! xcrun stapler staple "${dmg_path}"; then
    emit_stage_marker "final-dmg-stapling-failed"
    return 1
  fi
  emit_stage_marker "final-dmg-stapling-completed"
}

validate_release_artifacts() {
  local dmg_path="$1"
  emit_stage_marker "artifact-policy-validation-started"
  if ! "${VALIDATE_ARTIFACTS_SCRIPT}" \
    --app "${APP_PATH}" \
    --dmg "${dmg_path}" \
    --expected-arch "${EXPECTED_ARCH}"; then
    emit_stage_marker "artifact-policy-validation-failed"
    return 1
  fi
  emit_stage_marker "artifact-policy-validation-completed"
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --clean)
      CLEAN="true"
      shift
      ;;
    --skip-build)
      SKIP_BUILD="true"
      shift
      ;;
    --target)
      if [[ "$#" -lt 2 || -z "${2:-}" ]]; then
        echo "--target requires a target triple" >&2
        usage
        exit 1
      fi
      BUILD_TARGET="$2"
      shift 2
      ;;
    --expected-arch)
      if [[ "$#" -lt 2 || -z "${2:-}" ]]; then
        echo "--expected-arch requires aarch64 or x86_64" >&2
        usage
        exit 1
      fi
      EXPECTED_ARCH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -n "${BUILD_TARGET}" ]]; then
  RELEASE_TARGET_DIR="${PROJECT_ROOT}/src-tauri/target/${BUILD_TARGET}/release"
else
  RELEASE_TARGET_DIR="${PROJECT_ROOT}/src-tauri/target/release"
fi

case "${EXPECTED_ARCH}" in
  ""|aarch64|x86_64) ;;
  *) echo "--expected-arch must be aarch64 or x86_64" >&2; exit 1 ;;
esac
infer_expected_arch

APP_PATH="${RELEASE_TARGET_DIR}/bundle/macos/RalphX.app"
MACOS_BUNDLE_DIR="${RELEASE_TARGET_DIR}/bundle/macos"
DMG_DIR="${RELEASE_TARGET_DIR}/bundle/dmg"
BIN_PATH="${RELEASE_TARGET_DIR}/ralphx"

prepare_trace_dir
trap handle_release_error ERR

if [[ "${GITHUB_ACTIONS:-false}" == "true" ]]; then
  CLEAN="true"
fi

if [[ "${CLEAN}" == "true" ]]; then
  emit_stage_marker "release-clean-started"
  echo "Cleaning previous release bundle artifacts..."
  rm -rf "${RELEASE_TARGET_DIR}/bundle"
  emit_stage_marker "release-clean-completed"
fi

emit_stage_marker "release-script-started"

if [[ "${SKIP_BUILD}" != "true" ]]; then
  emit_stage_marker "runtime-preparation-started"
  echo "Building RalphX production release artifacts..."
  "${PREPARE_RUNTIME_SCRIPT}"
  emit_stage_marker "runtime-preparation-completed"
  run_tauri_release_build
else
  emit_stage_marker "release-build-skipped"
fi

DMG_PATH="$(resolve_single_dmg)"
if [[ "${SKIP_BUILD}" != "true" ]]; then
  notarize_final_dmg "${DMG_PATH}"
fi
validate_release_artifacts "${DMG_PATH}"

missing_artifacts="false"

echo ""
echo "Production release artifact summary"
echo "---------------------------------"
echo "Local app data was not modified."
echo "Application Support plugin/runtime sync was not performed."
if [[ -n "${BUILD_TARGET}" ]]; then
  echo "Build target: ${BUILD_TARGET}"
fi
echo "Expected architecture: ${EXPECTED_ARCH}"
echo "Release trace log: ${RAW_TRACE_LOG}"
echo "Release stage log: ${STAGE_TRACE_LOG}"
emit_stage_marker "artifact-summary-started"

if [[ -x "${BIN_PATH}" ]]; then
  echo "Binary: ${BIN_PATH}"
fi

if [[ -d "${APP_PATH}" ]]; then
  echo "App bundle: ${APP_PATH}"
else
  echo "App bundle not found: ${APP_PATH}" >&2
  missing_artifacts="true"
fi

if [[ -d "${MACOS_BUNDLE_DIR}" ]]; then
  echo "Updater bundle directory: ${MACOS_BUNDLE_DIR}"
  updater_count=0
  while IFS= read -r updater_path; do
    [[ -n "${updater_path}" ]] || continue
    echo "${updater_path}"
    if [[ ! -f "${updater_path}.sig" ]]; then
      echo "Updater signature not found for: ${updater_path}" >&2
      missing_artifacts="true"
      continue
    fi
    echo "${updater_path}.sig"
    updater_count=$((updater_count + 1))
  done < <(find "${MACOS_BUNDLE_DIR}" -maxdepth 1 -type f -name '*.app.tar.gz' -print)
  if [[ "${updater_count}" -eq 0 ]]; then
    echo "No updater bundles found under: ${MACOS_BUNDLE_DIR}" >&2
    missing_artifacts="true"
  fi
else
  echo "Updater bundle directory not found: ${MACOS_BUNDLE_DIR}" >&2
  missing_artifacts="true"
fi

echo "DMG directory: ${DMG_DIR}"
echo "${DMG_PATH}"

echo ""
echo "Next step:"
echo "  Release artifacts passed signature, notarization, Gatekeeper, metadata, and architecture checks."

if [[ "${missing_artifacts}" == "true" ]]; then
  emit_stage_marker "artifact-summary-failed"
  exit 1
fi

emit_stage_marker "release-script-completed"
