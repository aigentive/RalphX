#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Validate existing RalphX macOS release artifacts without publishing or notarizing them.

Usage:
  ./scripts/validate-macos-release-artifacts.sh \
    --app <RalphX.app> \
    --dmg <RalphX.dmg> \
    --expected-arch <aarch64|x86_64>

The app and DMG paths must identify the exact artifacts to inspect. This command
does not submit artifacts to Apple, remove quarantine, or modify app data.
EOF
}

APP_PATH=""
DMG_PATH=""
EXPECTED_ARCH=""

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --app)
      [[ "$#" -ge 2 && -n "${2:-}" ]] || { echo "--app requires a path" >&2; exit 2; }
      APP_PATH="$2"
      shift 2
      ;;
    --dmg)
      [[ "$#" -ge 2 && -n "${2:-}" ]] || { echo "--dmg requires a path" >&2; exit 2; }
      DMG_PATH="$2"
      shift 2
      ;;
    --expected-arch)
      [[ "$#" -ge 2 && -n "${2:-}" ]] || { echo "--expected-arch requires a value" >&2; exit 2; }
      EXPECTED_ARCH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ -n "${APP_PATH}" ]] || { echo "Missing required --app path" >&2; exit 2; }
[[ -n "${DMG_PATH}" ]] || { echo "Missing required --dmg path" >&2; exit 2; }
case "${EXPECTED_ARCH}" in
  aarch64) EXPECTED_MACHO_ARCH="arm64" ;;
  x86_64) EXPECTED_MACHO_ARCH="x86_64" ;;
  *)
    echo "--expected-arch must be aarch64 or x86_64" >&2
    exit 2
    ;;
esac

[[ -d "${APP_PATH}" ]] || { echo "App bundle not found: ${APP_PATH}" >&2; exit 1; }
[[ -f "${DMG_PATH}" ]] || { echo "DMG not found: ${DMG_PATH}" >&2; exit 1; }

require_tool() {
  local tool="$1"
  command -v "${tool}" >/dev/null 2>&1 || {
    echo "Required validation tool not found: ${tool}" >&2
    exit 1
  }
}

for tool in codesign xcrun spctl plutil lipo; do
  require_tool "${tool}"
done

run_check() {
  local label="$1"
  shift
  echo "[artifact-validation] ${label}"
  if ! "$@"; then
    echo "Artifact validation failed: ${label}" >&2
    exit 1
  fi
}

read_plist_value() {
  local key="$1"
  local plist_path="$2"
  plutil -extract "${key}" raw "${plist_path}" 2>/dev/null
}

INFO_PLIST="${APP_PATH}/Contents/Info.plist"
[[ -f "${INFO_PLIST}" ]] || { echo "App Info.plist not found: ${INFO_PLIST}" >&2; exit 1; }

run_check "app signature" codesign --verify --deep --strict --verbose=2 "${APP_PATH}"
run_check "app stapled ticket" xcrun stapler validate "${APP_PATH}"
run_check "app Gatekeeper executable assessment" spctl --assess --type execute --verbose=4 "${APP_PATH}"
if command -v syspolicy_check >/dev/null 2>&1; then
  run_check "app distribution policy" syspolicy_check distribution "${APP_PATH}"
else
  echo "[artifact-validation] app distribution policy skipped (syspolicy_check unavailable)"
fi

bundle_identifier="$(read_plist_value CFBundleIdentifier "${INFO_PLIST}" || true)"
[[ "${bundle_identifier}" == "com.ralphx.app" ]] || {
  echo "Unexpected CFBundleIdentifier: ${bundle_identifier:-missing}" >&2
  exit 1
}

executable_name="$(read_plist_value CFBundleExecutable "${INFO_PLIST}" || true)"
[[ "${executable_name}" == "ralphx" ]] || {
  echo "Unexpected CFBundleExecutable: ${executable_name:-missing}" >&2
  exit 1
}

minimum_system="$(read_plist_value LSMinimumSystemVersion "${INFO_PLIST}" || true)"
[[ "${minimum_system}" == "13.0" ]] || {
  echo "Unexpected LSMinimumSystemVersion: ${minimum_system:-missing}; expected 13.0" >&2
  exit 1
}

if requires_carbon="$(read_plist_value LSRequiresCarbon "${INFO_PLIST}" 2>/dev/null)"; then
  echo "[artifact-validation] LSRequiresCarbon=${requires_carbon} (diagnostic only)"
fi

EXECUTABLE_PATH="${APP_PATH}/Contents/MacOS/${executable_name}"
[[ -x "${EXECUTABLE_PATH}" ]] || { echo "App executable not found: ${EXECUTABLE_PATH}" >&2; exit 1; }
actual_arch="$(lipo -archs "${EXECUTABLE_PATH}" | xargs)"
[[ "${actual_arch}" == "${EXPECTED_MACHO_ARCH}" ]] || {
  echo "App architecture mismatch: expected ${EXPECTED_MACHO_ARCH}, found ${actual_arch:-unknown}" >&2
  exit 1
}
echo "[artifact-validation] app metadata identifier=${bundle_identifier} executable=${executable_name} minimum_system=${minimum_system} architecture=${actual_arch}"

run_check "DMG signature" codesign --verify --strict --verbose=2 "${DMG_PATH}"
run_check "DMG stapled ticket" xcrun stapler validate "${DMG_PATH}"
run_check "DMG Gatekeeper open assessment" \
  spctl --assess --type open --context context:primary-signature --verbose=4 "${DMG_PATH}"

echo "[artifact-validation] completed"
