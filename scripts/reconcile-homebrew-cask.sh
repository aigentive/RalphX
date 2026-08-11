#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Reconcile an already-rendered RalphX Homebrew cask into a tap repository.

Usage:
  ./scripts/reconcile-homebrew-cask.sh \
    --repo-url <authenticated-or-local-git-url> \
    --expected-cask <path> \
    --tag <vX.Y.Z> \
    --work-dir <path>

The cask must be rendered and staged before this script runs. This script commits only an
exact changed Casks/ralphx.rb, pushes it to main, then verifies the remote byte-for-byte.
EOF
}

repo_url=""
expected_cask=""
tag=""
work_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url)
      repo_url="${2:-}"
      shift 2
      ;;
    --expected-cask)
      expected_cask="${2:-}"
      shift 2
      ;;
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --work-dir)
      work_dir="${2:-}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

die() {
  echo "Homebrew cask reconciliation: $*" >&2
  exit 1
}

for required in repo_url expected_cask tag work_dir; do
  [[ -n "${!required}" ]] || die "--${required//_/-} is required."
done
[[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || die "--tag must be an exact vX.Y.Z release tag."
[[ -f "${expected_cask}" ]] || die "Staged expected cask is missing: ${expected_cask}"
expected_cask="$(cd "$(dirname "${expected_cask}")" && pwd -P)/$(basename "${expected_cask}")"

mkdir -p "${work_dir}"
tap_dir="$(mktemp -d "${work_dir}/tap.XXXXXX")"
cleanup() {
  rm -rf "${tap_dir}"
}
trap cleanup EXIT

git clone --quiet "${repo_url}" "${tap_dir}"
mkdir -p "${tap_dir}/Casks"
if ! cmp -s "${expected_cask}" "${tap_dir}/Casks/ralphx.rb"; then
  cp "${expected_cask}" "${tap_dir}/Casks/ralphx.rb"
fi

cd "${tap_dir}"
if [[ -n "$(git status --porcelain --untracked-files=all -- Casks/ralphx.rb)" ]]; then
  git config user.name "github-actions[bot]"
  git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
  git add Casks/ralphx.rb
  git commit -m "chore: update ralphx cask to ${tag}"
  git push origin HEAD:main
fi

git fetch origin main
git show origin/main:Casks/ralphx.rb | cmp -s - "${expected_cask}" \
  || die "Published Homebrew cask does not exactly match the staged Stable cask for ${tag}."

echo "Homebrew cask converged for ${tag}."
