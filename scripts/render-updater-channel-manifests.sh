#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Render versioned and channel-pointer Tauri updater manifests from validated release inputs.

Usage:
  ./scripts/render-updater-channel-manifests.sh \
    --tag vX.Y.Z \
    --notes-file <path> \
    --aarch64-signature <path> \
    --x86_64-signature <path> \
    --pub-date <RFC3339 UTC timestamp> \
    --channel <nightly|stable> \
    --output-dir <path>

Outputs a normal versioned latest.json plus latest-aarch64.json and
latest-x86_64.json for the requested channel. Pointer URLs are always pinned to
the aigentive/ralphx.app GitHub Release for the supplied tag.
EOF
}

tag=""
notes_file=""
aarch64_signature=""
x86_64_signature=""
pub_date=""
channel=""
output_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --notes-file)
      notes_file="${2:-}"
      shift 2
      ;;
    --aarch64-signature)
      aarch64_signature="${2:-}"
      shift 2
      ;;
    --x86_64-signature)
      x86_64_signature="${2:-}"
      shift 2
      ;;
    --pub-date)
      pub_date="${2:-}"
      shift 2
      ;;
    --channel)
      channel="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
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

if [[ ! "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "--tag must be an exact vX.Y.Z release tag" >&2
  exit 1
fi

if [[ ! "${pub_date}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
  echo "--pub-date must be an RFC3339 UTC timestamp" >&2
  exit 1
fi

case "${channel}" in
  nightly|stable)
    ;;
  *)
    echo "--channel must be nightly or stable" >&2
    exit 1
    ;;
esac

for path in "${notes_file}" "${aarch64_signature}" "${x86_64_signature}"; do
  if [[ ! -f "${path}" || ! -r "${path}" ]]; then
    echo "Required readable file is missing: ${path}" >&2
    exit 1
  fi
done
for path in "${aarch64_signature}" "${x86_64_signature}"; do
  if [[ ! -s "${path}" ]]; then
    echo "Updater signature must be non-empty: ${path}" >&2
    exit 1
  fi
done

if [[ -z "${output_dir}" ]]; then
  echo "--output-dir is required" >&2
  exit 1
fi

version="${tag#v}"
release_base="https://github.com/aigentive/ralphx.app/releases/download/${tag}"
aarch64_url="${release_base}/RalphX_${version}_aarch64.app.tar.gz"
x86_64_url="${release_base}/RalphX_${version}_x86_64.app.tar.gz"

mkdir -p "${output_dir}"

render_manifest() {
  local destination="$1"
  local target="$2"
  local url="$3"
  local signature_file="$4"

  jq -n \
    --arg version "${tag}" \
    --rawfile notes "${notes_file}" \
    --arg pub_date "${pub_date}" \
    --arg target "${target}" \
    --arg url "${url}" \
    --rawfile signature "${signature_file}" \
    '{
      version: $version,
      notes: $notes,
      pub_date: $pub_date,
      platforms: {
        ($target): {
          url: $url,
          signature: $signature
        }
      }
    }' >"${destination}"
}

jq -n \
  --arg version "${tag}" \
  --rawfile notes "${notes_file}" \
  --arg pub_date "${pub_date}" \
  --arg aarch64_url "${aarch64_url}" \
  --rawfile aarch64_signature "${aarch64_signature}" \
  --arg x86_64_url "${x86_64_url}" \
  --rawfile x86_64_signature "${x86_64_signature}" \
  '{
    version: $version,
    notes: $notes,
    pub_date: $pub_date,
    platforms: {
      "darwin-aarch64": {
        url: $aarch64_url,
        signature: $aarch64_signature
      },
      "darwin-x86_64": {
        url: $x86_64_url,
        signature: $x86_64_signature
      }
    }
  }' >"${output_dir}/latest.json"

render_manifest \
  "${output_dir}/latest-aarch64.json" \
  "${channel}" \
  "${aarch64_url}" \
  "${aarch64_signature}"
render_manifest \
  "${output_dir}/latest-x86_64.json" \
  "${channel}" \
  "${x86_64_url}" \
  "${x86_64_signature}"
