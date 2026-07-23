#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Verify the public GitHub download URLs for a fixed updater pointer release.

Usage:
  ./scripts/verify-public-updater-pointers.sh <owner/repo> <updater-stable|updater-nightly> <expected-pointer-dir>

The helper retries GitHub's public asset cache for at most five minutes. Set
RELEASE_PUBLIC_VERIFY_ATTEMPTS and RELEASE_PUBLIC_VERIFY_DELAY_SECONDS for deterministic tests.
EOF
}

if [[ $# -ne 3 ]]; then
  usage >&2
  exit 1
fi

repo="$1"
pointer_tag="$2"
expected_dir="$3"

[[ "${repo}" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] || {
  echo "Repository must be an exact owner/repo value." >&2
  exit 1
}
[[ "${pointer_tag}" =~ ^updater-(stable|nightly)$ ]] || {
  echo "Pointer tag must be updater-stable or updater-nightly." >&2
  exit 1
}
[[ -d "${expected_dir}" ]] || {
  echo "Expected pointer directory is missing: ${expected_dir}" >&2
  exit 1
}

attempts="${RELEASE_PUBLIC_VERIFY_ATTEMPTS:-10}"
delay_seconds="${RELEASE_PUBLIC_VERIFY_DELAY_SECONDS:-10}"
[[ "${attempts}" =~ ^[1-9][0-9]*$ && "${attempts}" -le 10 ]] || {
  echo "RELEASE_PUBLIC_VERIFY_ATTEMPTS must be an integer from 1 through 10." >&2
  exit 1
}
[[ "${delay_seconds}" =~ ^[0-9]+$ && "${delay_seconds}" -le 10 ]] || {
  echo "RELEASE_PUBLIC_VERIFY_DELAY_SECONDS must be an integer from 0 through 10." >&2
  exit 1
}
# curl is capped at 20 seconds; retries plus sleeps must not exceed five minutes.
(( attempts * 20 + (attempts - 1) * delay_seconds <= 300 )) || {
  echo "Public updater-pointer retry budget exceeds five minutes." >&2
  exit 1
}

verify_asset() {
  local asset="$1"
  local expected="${expected_dir}/${asset}"
  local actual="${expected_dir}/.public-${asset}"
  local attempt=1

  [[ -r "${expected}" && -s "${expected}" ]] || {
    echo "Expected pointer asset is missing: ${expected}" >&2
    exit 1
  }

  while true; do
    rm -f "${actual}"
    if curl --fail --location --silent --show-error \
      --connect-timeout 5 \
      --max-time 20 \
      --output "${actual}" \
      "https://github.com/${repo}/releases/download/${pointer_tag}/${asset}" \
      && cmp -s "${actual}" "${expected}"; then
      rm -f "${actual}"
      return
    fi

    rm -f "${actual}"
    if [[ "${attempt}" -ge "${attempts}" ]]; then
      echo "Public ${pointer_tag} pointer ${asset} did not converge after ${attempts} bounded attempts." >&2
      exit 1
    fi
    printf 'Public %s pointer %s is stale or unavailable (attempt %s/%s); retrying.\n' \
      "${pointer_tag}" "${asset}" "${attempt}" "${attempts}" >&2
    if [[ "${delay_seconds}" -gt 0 ]]; then
      sleep "${delay_seconds}"
    fi
    attempt=$((attempt + 1))
  done
}

verify_asset latest-aarch64.json
verify_asset latest-x86_64.json

echo "Public ${pointer_tag} updater pointers match the GitHub API assets."
