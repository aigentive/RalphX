#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
TAP_BARE="${TEST_TMP}/tap.git"
TAP_SEED="${TEST_TMP}/tap-seed"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_equals() {
  local expected="$1"
  local actual="$2"
  [[ "${actual}" == "${expected}" ]] || fail "expected '${expected}', got '${actual}'"
}

assert_fails() {
  local label="$1"
  shift
  if "$@" >"${TEST_TMP}/${label}.log" 2>&1; then
    fail "${label} unexpectedly succeeded"
  fi
}

remote_cask() {
  git --git-dir="${TAP_BARE}" show main:Casks/ralphx.rb
}

remote_commit_count() {
  git --git-dir="${TAP_BARE}" rev-list --count main
}

run_reconcile() {
  local tag="$1"
  local cask_file="$2"
  local work_dir="$3"
  bash "${ROOT_DIR}/scripts/reconcile-homebrew-cask.sh" \
    --repo-url "${TAP_BARE}" \
    --expected-cask "${cask_file}" \
    --tag "${tag}" \
    --work-dir "${work_dir}"
}

git init --bare --initial-branch=main "${TAP_BARE}" >/dev/null
git clone "${TAP_BARE}" "${TAP_SEED}" >/dev/null
mkdir -p "${TAP_SEED}/Casks"
printf 'old cask\n' >"${TAP_SEED}/Casks/ralphx.rb"
git -C "${TAP_SEED}" config user.name "fixture"
git -C "${TAP_SEED}" config user.email "fixture@example.test"
git -C "${TAP_SEED}" add Casks/ralphx.rb
git -C "${TAP_SEED}" commit -m "fixture: initial cask" >/dev/null
git -C "${TAP_SEED}" push origin HEAD:main >/dev/null

cat >"${TAP_BARE}/hooks/pre-receive" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ -e "$(dirname "$0")/../reject-push" ]]; then
  exit 1
fi
EOF
chmod +x "${TAP_BARE}/hooks/pre-receive"

FIRST_CASK="${TEST_TMP}/ralphx-0.77.0.rb"
SECOND_CASK="${TEST_TMP}/ralphx-0.78.0.rb"
bash "${ROOT_DIR}/scripts/render-homebrew-cask.sh" \
  0.77.0 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb >"${FIRST_CASK}"
bash "${ROOT_DIR}/scripts/render-homebrew-cask.sh" \
  0.78.0 cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc \
  dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd >"${SECOND_CASK}"

# First reconciliation commits and pushes the exact rendered cask.
run_reconcile v0.77.0 "${FIRST_CASK}" "${TEST_TMP}/first"
cmp -s "${FIRST_CASK}" <(remote_cask) || fail "initial cask did not converge"
assert_equals 2 "$(remote_commit_count)"

# An exact rerun is a no-op: it neither commits nor pushes another cask revision.
run_reconcile v0.77.0 "${FIRST_CASK}" "${TEST_TMP}/rerun"
cmp -s "${FIRST_CASK}" <(remote_cask) || fail "idempotent rerun changed the cask"
assert_equals 2 "$(remote_commit_count)"

# The workflow passes its staged cask and work directory as paths relative to its workspace.
WORKFLOW_DIR="${TEST_TMP}/workflow"
mkdir -p "${WORKFLOW_DIR}/stable-control"
cp "${FIRST_CASK}" "${WORKFLOW_DIR}/stable-control/staged-homebrew-cask.rb"
(
  cd "${WORKFLOW_DIR}"
  run_reconcile \
    v0.77.0 \
    stable-control/staged-homebrew-cask.rb \
    stable-homebrew
)
assert_equals 2 "$(remote_commit_count)"

# A rejected push fails without reporting success or changing the remote authority.
touch "${TAP_BARE}/reject-push"
assert_fails rejects_push run_reconcile v0.78.0 "${SECOND_CASK}" "${TEST_TMP}/rejected"
cmp -s "${FIRST_CASK}" <(remote_cask) || fail "failed push changed the remote cask"
assert_equals 2 "$(remote_commit_count)"

# A later exact rerun repairs the failed publication and verifies the remote byte-for-byte.
rm "${TAP_BARE}/reject-push"
run_reconcile v0.78.0 "${SECOND_CASK}" "${TEST_TMP}/repair"
cmp -s "${SECOND_CASK}" <(remote_cask) || fail "repair rerun did not converge"
assert_equals 3 "$(remote_commit_count)"

echo "PASS: Homebrew cask reconciliation converges locally, is idempotent, and fails closed on push rejection"
