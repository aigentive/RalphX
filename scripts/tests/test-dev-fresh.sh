#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "${TEST_TMP}"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_contains() {
  local expected="$1"
  local file="$2"
  grep -Fq -- "${expected}" "${file}" || fail "missing '${expected}' in ${file}"
}

BIN_DIR="${TEST_TMP}/bin"
PACKAGE_DIR="${TEST_TMP}/ralphx-mcp-server"
SHARED_MODULES="${TEST_TMP}/shared-node-modules"
NPM_LOG="${TEST_TMP}/npm.log"

mkdir -p "${BIN_DIR}" "${PACKAGE_DIR}" "${SHARED_MODULES}"
ln -s "${SHARED_MODULES}" "${PACKAGE_DIR}/node_modules"
printf '{"name":"fixture"}\n' >"${PACKAGE_DIR}/package.json"

cat >"${BIN_DIR}/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "$(pwd): npm $*" >>"${NPM_LOG}"
case "${1:-}" in
  ls)
    exit 0
    ;;
  ci)
    mkdir -p node_modules
    : >node_modules/materialized
    exit 0
    ;;
  *)
    exit 1
    ;;
esac
EOF
chmod +x "${BIN_DIR}/npm"

export NPM_LOG
export PATH="${BIN_DIR}:/usr/bin:/bin"
# shellcheck source=/dev/null
source "${ROOT_DIR}/dev-fresh"

ensure_package_install "${PACKAGE_DIR}" "fixture MCP"

[[ -d "${PACKAGE_DIR}/node_modules" ]] || fail "node_modules was not installed"
[[ ! -L "${PACKAGE_DIR}/node_modules" ]] || fail "node_modules symlink was not materialized"
[[ -f "${PACKAGE_DIR}/node_modules/materialized" ]] || fail "npm ci did not create the dependency marker"
assert_contains "${PACKAGE_DIR}: npm ci" "${NPM_LOG}"

echo "PASS: dev-fresh materializes symlinked MCP dependencies"
