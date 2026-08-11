#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Validate fixed release assets before or after a channel pointer change.

Usage:
  ./scripts/validate-release-promotion.sh <vX.Y.Z> <source-assets-dir> <pointer-assets-dir> <nightly|stable>

The helper is pure: it performs no network, Git, release, tag, or publishing action.
EOF
}

if [[ $# -ne 4 ]]; then
  usage >&2
  exit 1
fi

tag="$1"
source_dir="$2"
pointer_dir="$3"
channel="$4"

if [[ ! "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Release version must be an exact vX.Y.Z tag: ${tag}" >&2
  exit 1
fi

case "${channel}" in
  nightly|stable)
    ;;
  *)
    echo "Channel must be nightly or stable: ${channel}" >&2
    exit 1
    ;;
esac

for path in "${source_dir}" "${pointer_dir}"; do
  if [[ ! -d "${path}" ]]; then
    echo "Asset directory is missing: ${path}" >&2
    exit 1
  fi
done

version="${tag#v}"
release_base="https://github.com/aigentive/ralphx.app/releases/download/${tag}"

source_asset_names=(
  "RalphX_${version}_aarch64.app.tar.gz"
  "RalphX_${version}_aarch64.app.tar.gz.sig"
  "RalphX_${version}_aarch64.dmg"
  "RalphX_${version}_x86_64.app.tar.gz"
  "RalphX_${version}_x86_64.app.tar.gz.sig"
  "RalphX_${version}_x86_64.dmg"
  "checksums.txt"
  "latest.json"
)
pointer_asset_names=(
  "latest-aarch64.json"
  "latest-x86_64.json"
)

assert_exact_asset_allowlist() {
  local directory="$1"
  shift
  local -a expected=("$@")
  local actual
  local expected_listing

  actual="$(find "${directory}" -maxdepth 1 -type f -exec basename {} \; | LC_ALL=C sort)"
  expected_listing="$(printf '%s\n' "${expected[@]}" | LC_ALL=C sort)"
  if [[ "${actual}" != "${expected_listing}" ]]; then
    echo "Unexpected asset set in ${directory}. Expected:" >&2
    printf '%s\n' "${expected_listing}" >&2
    echo "Actual:" >&2
    printf '%s\n' "${actual}" >&2
    exit 1
  fi

  local filename
  for filename in "${expected[@]}"; do
    if [[ ! -r "${directory}/${filename}" || ! -s "${directory}/${filename}" ]]; then
      echo "Required readable non-empty asset is missing: ${directory}/${filename}" >&2
      exit 1
    fi
  done
}

source_asset_listing="$(find "${source_dir}" -maxdepth 1 -type f -exec basename {} \; | LC_ALL=C sort)"
base_source_asset_listing="$(printf '%s\n' "${source_asset_names[@]}" | LC_ALL=C sort)"
if [[ "${source_asset_listing}" != "${base_source_asset_listing}" ]]; then
  echo "Unexpected source release asset set in ${source_dir}." >&2
  exit 1
fi
for filename in "${source_asset_names[@]}"; do
  if [[ ! -r "${source_dir}/${filename}" || ! -s "${source_dir}/${filename}" ]]; then
    echo "Required readable non-empty asset is missing: ${source_dir}/${filename}" >&2
    exit 1
  fi
done

assert_manifest_target() {
  local manifest="$1"
  local target="$2"
  local expected_url="$3"
  local signature_file="$4"
  local signature_output

  jq -e \
    --arg tag "${tag}" \
    --arg target "${target}" \
    --arg url "${expected_url}" \
    '(.version == $tag)
      and (.platforms[$target] | type == "object")
      and (.platforms[$target].url == $url)
      and (.platforms[$target].signature | type == "string")' \
    "${manifest}" >/dev/null || {
      echo "${manifest} does not contain the fixed ${target} GitHub release URL for ${tag}" >&2
      exit 1
    }

  signature_output="$(mktemp)"
  jq -jr --arg target "${target}" '.platforms[$target].signature' "${manifest}" >"${signature_output}"
  if ! cmp -s "${signature_output}" "${signature_file}"; then
    rm -f "${signature_output}"
    echo "${manifest} ${target} signature does not byte-match ${signature_file}" >&2
    exit 1
  fi
  rm -f "${signature_output}"
}

assert_versioned_manifest() {
  local manifest="${source_dir}/latest.json"
  local arm_archive="RalphX_${version}_aarch64.app.tar.gz"
  local intel_archive="RalphX_${version}_x86_64.app.tar.gz"

  jq -e \
    --arg tag "${tag}" \
    '(.version == $tag)
      and ((.platforms | keys | sort) == ["darwin-aarch64", "darwin-x86_64"])
      and (.notes | type == "string")
      and (.pub_date | type == "string")' \
    "${manifest}" >/dev/null || {
      echo "${manifest} is not a complete Tauri updater manifest for ${tag}" >&2
      exit 1
    }

  assert_manifest_target \
    "${manifest}" \
    "darwin-aarch64" \
    "${release_base}/${arm_archive}" \
    "${source_dir}/${arm_archive}.sig"
  assert_manifest_target \
    "${manifest}" \
    "darwin-x86_64" \
    "${release_base}/${intel_archive}" \
    "${source_dir}/${intel_archive}.sig"
}

assert_versioned_manifest

assert_exact_asset_allowlist "${pointer_dir}" "${pointer_asset_names[@]}"
jq -e --arg channel "${channel}" '(.platforms | keys) == [$channel]' \
  "${pointer_dir}/latest-aarch64.json" >/dev/null || {
  echo "${channel} aarch64 pointer must contain only the ${channel} platform key" >&2
  exit 1
}
assert_manifest_target \
  "${pointer_dir}/latest-aarch64.json" \
  "${channel}" \
  "${release_base}/RalphX_${version}_aarch64.app.tar.gz" \
  "${source_dir}/RalphX_${version}_aarch64.app.tar.gz.sig"
jq -e --arg channel "${channel}" '(.platforms | keys) == [$channel]' \
  "${pointer_dir}/latest-x86_64.json" >/dev/null || {
  echo "${channel} x86_64 pointer must contain only the ${channel} platform key" >&2
  exit 1
}
assert_manifest_target \
  "${pointer_dir}/latest-x86_64.json" \
  "${channel}" \
  "${release_base}/RalphX_${version}_x86_64.app.tar.gz" \
  "${source_dir}/RalphX_${version}_x86_64.app.tar.gz.sig"

echo "Validated ${tag} source assets and ${channel} channel assets."
