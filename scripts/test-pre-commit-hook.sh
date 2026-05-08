#!/usr/bin/env bash
# Tests for .githooks/pre-commit
# Verifies the hook rejects node_modules paths, rejects staged Tauri version drift,
# and allows normal files.

set -uo pipefail

HOOK_DIR="$(cd "$(dirname "$0")/.." && pwd)/.githooks"
PASS=0
FAIL=0
FAKE_BIN=""
FAKE_NPM_LOG=""
FAKE_ESLINT_LOG=""

# ─── helpers ────────────────────────────────────────────────────────────────

setup_repo() {
  local dir
  dir=$(mktemp -d)
  git -C "$dir" init -q
  git -C "$dir" config user.email "test@test.com"
  git -C "$dir" config user.name "Test"
  git -C "$dir" config core.hooksPath "$HOOK_DIR"
  echo "$dir"
}

pass() { echo "  PASS: $1"; ((PASS++)); }
fail() { echo "  FAIL: $1"; ((FAIL++)); }

setup_fake_node_tools() {
  FAKE_BIN=$(mktemp -d)
  FAKE_NPM_LOG=$(mktemp)
  FAKE_ESLINT_LOG=$(mktemp)
  export FAKE_NPM_LOG FAKE_ESLINT_LOG

  cat > "$FAKE_BIN/node" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
  chmod +x "$FAKE_BIN/node"

  cat > "$FAKE_BIN/npm" <<'EOF'
#!/usr/bin/env bash
echo "npm $*" >> "$FAKE_NPM_LOG"
case "$1" in
  ci|install)
    mkdir -p node_modules/.bin
    cat > node_modules/.bin/eslint <<'EOF_ESLINT'
#!/usr/bin/env bash
echo "eslint $*" >> "$FAKE_ESLINT_LOG"
exit 0
EOF_ESLINT
    chmod +x node_modules/.bin/eslint
    ;;
esac
exit 0
EOF
  chmod +x "$FAKE_BIN/npm"
}

setup_fake_nvm_home() {
  local version="$1"
  local home dir

  home=$(mktemp -d)
  dir="$home/.nvm/versions/node/v$version/bin"
  mkdir -p "$dir"

  cat > "$dir/node" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
  chmod +x "$dir/node"

  cat > "$dir/npm" <<'EOF'
#!/usr/bin/env bash
echo "npm $*" >> "$FAKE_NPM_LOG"
case "$1" in
  ci|install)
    mkdir -p node_modules/.bin
    cat > node_modules/.bin/eslint <<'EOF_ESLINT'
#!/usr/bin/env bash
echo "eslint $*" >> "$FAKE_ESLINT_LOG"
exit 0
EOF_ESLINT
    chmod +x node_modules/.bin/eslint
    ;;
esac
exit 0
EOF
  chmod +x "$dir/npm"

  echo "$home"
}

write_frontend_validation_fixture() {
  local dir="$1"

  mkdir -p "$dir/frontend/src" "$dir/frontend/scripts" "$dir/frontend/node_modules/.bin" "$dir/src-tauri"
  echo "frontend/node_modules" > "$dir/.gitignore"

  cat > "$dir/frontend/src/index.ts" <<'EOF'
export const x = 1;
EOF

  cat > "$dir/frontend/scripts/check-design-tokens.sh" <<'EOF'
#!/usr/bin/env bash
echo "token guard ok"
EOF
  chmod +x "$dir/frontend/scripts/check-design-tokens.sh"

  cat > "$dir/frontend/node_modules/.bin/eslint" <<'EOF'
#!/usr/bin/env bash
if [ -n "${FAKE_ESLINT_LOG:-}" ]; then
  echo "eslint $*" >> "$FAKE_ESLINT_LOG"
fi
exit 0
EOF
  chmod +x "$dir/frontend/node_modules/.bin/eslint"

  cat > "$dir/frontend/package.json" <<'EOF'
{
  "scripts": {
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.10.1"
  }
}
EOF

  cat > "$dir/frontend/package-lock.json" <<'EOF'
{
  "name": "test",
  "lockfileVersion": 3,
  "packages": {
    "": {
      "dependencies": {
        "@tauri-apps/api": "^2.10.1"
      }
    },
    "node_modules/@tauri-apps/api": { "version": "2.10.1" }
  }
}
EOF

  cat > "$dir/src-tauri/Cargo.toml" <<'EOF'
[dependencies]
tauri = { version = "2.10.3", features = ["devtools"] }
EOF

  cat > "$dir/src-tauri/Cargo.lock" <<'EOF'
[[package]]
name = "tauri"
version = "2.10.3"
EOF
}

setup_primary_with_worktree() {
  local root primary wt
  root=$(mktemp -d)
  primary="$root/primary"
  wt="$root/worktree"
  mkdir -p "$primary"
  git -C "$primary" init -q
  git -C "$primary" config user.email "test@test.com"
  git -C "$primary" config user.name "Test"
  git -C "$primary" config core.hooksPath "$HOOK_DIR"
  write_frontend_validation_fixture "$primary"
  git -C "$primary" add .
  git -C "$primary" commit --no-verify -m "initial" >/dev/null
  mkdir -p "$primary/frontend/node_modules/.bin"
  git -C "$primary" worktree add -q -b test-worktree "$wt" HEAD
  echo "$root|$primary|$wt"
}

write_aligned_tauri_files() {
  local dir="$1"

  mkdir -p "$dir/frontend" "$dir/src-tauri"

  cat > "$dir/frontend/package.json" <<'EOF'
{
  "dependencies": {
    "@tauri-apps/api": "^2.10.1",
    "@tauri-apps/plugin-dialog": "^2.7.0",
    "@tauri-apps/plugin-fs": "^2.5.0",
    "@tauri-apps/plugin-global-shortcut": "^2.3.1",
    "@tauri-apps/plugin-opener": "^2.5.3",
    "@tauri-apps/plugin-process": "^2.3.1",
    "@tauri-apps/plugin-updater": "^2.10.1"
  }
}
EOF

  cat > "$dir/frontend/package-lock.json" <<'EOF'
{
  "name": "test",
  "packages": {
    "": {
      "dependencies": {
        "@tauri-apps/api": "^2.10.1",
        "@tauri-apps/plugin-dialog": "^2.7.0",
        "@tauri-apps/plugin-fs": "^2.5.0",
        "@tauri-apps/plugin-global-shortcut": "^2.3.1",
        "@tauri-apps/plugin-opener": "^2.5.3",
        "@tauri-apps/plugin-process": "^2.3.1",
        "@tauri-apps/plugin-updater": "^2.10.1"
      }
    },
    "node_modules/@tauri-apps/api": { "version": "2.10.1" },
    "node_modules/@tauri-apps/plugin-dialog": { "version": "2.7.0" },
    "node_modules/@tauri-apps/plugin-fs": { "version": "2.5.0" },
    "node_modules/@tauri-apps/plugin-global-shortcut": { "version": "2.3.1" },
    "node_modules/@tauri-apps/plugin-opener": { "version": "2.5.3" },
    "node_modules/@tauri-apps/plugin-process": { "version": "2.3.1" },
    "node_modules/@tauri-apps/plugin-updater": { "version": "2.10.1" }
  }
}
EOF

  cat > "$dir/src-tauri/Cargo.toml" <<'EOF'
[build-dependencies]
tauri-build = { version = "2.5.6", features = [] }

[dependencies]
tauri = { version = "2.10.3", features = ["devtools"] }
tauri-plugin-dialog = "2.7.0"
tauri-plugin-fs = "2.5.0"
tauri-plugin-global-shortcut = "2.3.1"
tauri-plugin-opener = "2.5.3"
tauri-plugin-updater = "2.10.1"
tauri-plugin-window-state = "2.4.1"
EOF

  cat > "$dir/src-tauri/Cargo.lock" <<'EOF'
[[package]]
name = "tauri"
version = "2.10.3"

[[package]]
name = "tauri-build"
version = "2.5.6"

[[package]]
name = "tauri-plugin-dialog"
version = "2.7.0"

[[package]]
name = "tauri-plugin-fs"
version = "2.5.0"

[[package]]
name = "tauri-plugin-global-shortcut"
version = "2.3.1"

[[package]]
name = "tauri-plugin-opener"
version = "2.5.3"

[[package]]
name = "tauri-plugin-updater"
version = "2.10.1"

[[package]]
name = "tauri-plugin-window-state"
version = "2.4.1"
EOF
}

# ─── Test 1: REJECT — top-level node_modules/package.json ───────────────────

T1=$(setup_repo)
mkdir -p "$T1/node_modules"
echo '{"name":"pkg"}' > "$T1/node_modules/package.json"
git -C "$T1" add -f node_modules/package.json
if git -C "$T1" commit -m "test" 2>/dev/null; then
  fail "Test 1: should have rejected node_modules/package.json"
else
  pass "Test 1: rejects top-level node_modules/package.json"
fi
rm -rf "$T1"

# ─── Test 2: REJECT — nested node_modules path ───────────────────────────────

T2=$(setup_repo)
mkdir -p "$T2/packages/lib/node_modules"
echo "// bar" > "$T2/packages/lib/node_modules/bar.js"
git -C "$T2" add -f packages/lib/node_modules/bar.js
if git -C "$T2" commit -m "test" 2>/dev/null; then
  fail "Test 2: should have rejected packages/lib/node_modules/bar.js"
else
  pass "Test 2: rejects nested packages/lib/node_modules/bar.js"
fi
rm -rf "$T2"

# ─── Test 3: ALLOW — normal source file ──────────────────────────────────────

T3=$(setup_repo)
mkdir -p "$T3/src"
echo "export const x = 1;" > "$T3/src/index.ts"
git -C "$T3" add src/index.ts
if git -C "$T3" commit -m "test" 2>/dev/null; then
  pass "Test 3: allows normal src/index.ts"
else
  fail "Test 3: should have allowed src/index.ts"
fi
rm -rf "$T3"

# ─── Test 4: ALLOW — file with 'node_modules' in non-directory context ───────

T4=$(setup_repo)
mkdir -p "$T4/docs"
echo "# Guide to node_modules" > "$T4/docs/node_modules_guide.md"
git -C "$T4" add docs/node_modules_guide.md
if git -C "$T4" commit -m "test" 2>/dev/null; then
  pass "Test 4: allows docs/node_modules_guide.md (no false positive)"
else
  fail "Test 4: should have allowed node_modules_guide.md"
fi
rm -rf "$T4"

# ─── Test 5: REJECT — staged Tauri version drift ────────────────────────────

T5=$(setup_repo)
write_aligned_tauri_files "$T5"
python3 - <<'PY' "$T5/frontend/package.json"
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
data = json.loads(path.read_text())
data["dependencies"]["@tauri-apps/api"] = "^2.11.0"
path.write_text(json.dumps(data, indent=2) + "\n")
PY
git -C "$T5" add frontend/package.json frontend/package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock
if git -C "$T5" commit -m "test" 2>/dev/null; then
  fail "Test 5: should have rejected staged Tauri version drift"
else
  pass "Test 5: rejects staged Tauri version drift"
fi
rm -rf "$T5"

# ─── Test 6: ALLOW — aligned staged Tauri version files ─────────────────────

T6=$(setup_repo)
write_aligned_tauri_files "$T6"
git -C "$T6" add frontend/package.json frontend/package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock
if git -C "$T6" commit -m "test" 2>/dev/null; then
  pass "Test 6: allows aligned staged Tauri version files"
else
  fail "Test 6: should have allowed aligned staged Tauri version files"
fi
rm -rf "$T6"

# ─── Test 7: ALLOW — frontend worktree reuses primary node_modules ──────────

setup_fake_node_tools
IFS='|' read -r T7 PRIMARY7 WT7 < <(setup_primary_with_worktree)
echo "export const y = 2;" >> "$WT7/frontend/src/index.ts"
git -C "$WT7" add frontend/src/index.ts
if PATH="$FAKE_BIN:$PATH" git -C "$WT7" commit -m "test" >/tmp/ralphx-hook-t7.out 2>&1; then
  if [ -L "$WT7/frontend/node_modules" ] &&
    [ "$(cd "$WT7/frontend/node_modules" && pwd -P)" = "$(cd "$PRIMARY7/frontend/node_modules" && pwd -P)" ] &&
    ! grep -Eq 'npm (ci|install)( |$)' "$FAKE_NPM_LOG" &&
    grep -Eq 'eslint --max-warnings=0 src/index.ts$' "$FAKE_ESLINT_LOG"; then
    pass "Test 7: frontend worktree symlinks primary node_modules without install"
  else
    fail "Test 7: expected primary node_modules symlink and no install"
  fi
else
  fail "Test 7: commit should have succeeded with primary node_modules fallback"
fi
rm -rf "$T7"

# ─── Test 8: ALLOW — changed dependency manifests install in worktree ───────

setup_fake_node_tools
IFS='|' read -r T8 _PRIMARY8 WT8 < <(setup_primary_with_worktree)
python3 - <<'PY' "$WT8/frontend/package.json" "$WT8/frontend/package-lock.json"
import json
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
lock_path = Path(sys.argv[2])
manifest = json.loads(manifest_path.read_text())
manifest.setdefault("dependencies", {})["left-pad"] = "^1.3.0"
manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")

lock = json.loads(lock_path.read_text())
lock.setdefault("packages", {})[""]["dependencies"]["left-pad"] = "^1.3.0"
lock["packages"]["node_modules/left-pad"] = {"version": "1.3.0"}
lock_path.write_text(json.dumps(lock, indent=2) + "\n")
PY
echo "export const z = 3;" >> "$WT8/frontend/src/index.ts"
git -C "$WT8" add frontend/package.json frontend/package-lock.json frontend/src/index.ts
if PATH="$FAKE_BIN:$PATH" git -C "$WT8" commit -m "test" >/tmp/ralphx-hook-t8.out 2>&1; then
  if [ -d "$WT8/frontend/node_modules" ] &&
    [ ! -L "$WT8/frontend/node_modules" ] &&
    grep -Eq 'npm ci --prefer-offline --no-audit --no-fund$' "$FAKE_NPM_LOG" &&
    grep -Eq 'eslint --max-warnings=0 src/index.ts$' "$FAKE_ESLINT_LOG"; then
    pass "Test 8: dependency manifest changes install worktree-local node_modules"
  else
    fail "Test 8: expected npm ci and worktree-local node_modules"
  fi
else
  fail "Test 8: commit should have succeeded after worktree-local install"
fi
rm -rf "$T8"

# ─── Test 9: REJECT — no dependency source available ────────────────────────

setup_fake_node_tools
IFS='|' read -r T9 PRIMARY9 WT9 < <(setup_primary_with_worktree)
rm -rf "$PRIMARY9/frontend/node_modules"
echo "export const missingDeps = true;" >> "$WT9/frontend/src/index.ts"
git -C "$WT9" add frontend/src/index.ts
if PATH="$FAKE_BIN:$PATH" git -C "$WT9" commit -m "test" >/tmp/ralphx-hook-t9.out 2>&1; then
  fail "Test 9: should fail when no frontend node_modules source exists"
else
  if grep -q "frontend/node_modules is missing" /tmp/ralphx-hook-t9.out; then
    pass "Test 9: missing frontend dependencies fail with clear infrastructure error"
  else
    fail "Test 9: missing dependency failure message was not clear"
  fi
fi
rm -rf "$T9"

# ─── Test 10: ALLOW — stripped PATH resolves .nvmrc-managed Node tools ──────

setup_fake_node_tools
T10=$(setup_repo)
write_frontend_validation_fixture "$T10"
echo "22.16.0" > "$T10/.nvmrc"
FAKE_HOME10=$(setup_fake_nvm_home "22.16.0")
FAKE_SYSTEM_BIN10=$(mktemp -d)
for tool in git dirname grep sed head tr tail; do
  ln -s "/usr/bin/$tool" "$FAKE_SYSTEM_BIN10/$tool"
done
echo "export const stripped = 10;" >> "$T10/frontend/src/index.ts"
git -C "$T10" add frontend/package.json frontend/package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock frontend/src/index.ts
if HOME="$FAKE_HOME10" NVM_BIN="$FAKE_HOME10/.nvm/versions/node/v22.16.0/bin" PATH="$FAKE_SYSTEM_BIN10:/bin" git -C "$T10" commit -m "test" >/tmp/ralphx-hook-t10.out 2>&1; then
  if grep -Eq 'npm ci --prefer-offline --no-audit --no-fund$' "$FAKE_NPM_LOG" &&
    grep -Eq 'npm run -s typecheck$' "$FAKE_NPM_LOG" &&
    grep -Eq 'eslint --max-warnings=0 src/index.ts$' "$FAKE_ESLINT_LOG"; then
    pass "Test 10: stripped PATH uses .nvmrc-backed Node tooling"
  else
    fail "Test 10: expected .nvmrc-resolved npm/typecheck/eslint invocations"
  fi
else
  fail "Test 10: commit should have succeeded with .nvmrc-backed Node tooling"
fi
rm -rf "$T10" "$FAKE_HOME10" "$FAKE_SYSTEM_BIN10"

# ─── Results ─────────────────────────────────────────────────────────────────

echo ""
echo "Results: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
