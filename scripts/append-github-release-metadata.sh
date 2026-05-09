#!/usr/bin/env bash

set -euo pipefail

START_MARKER="<!-- github-release-metadata:start -->"
END_MARKER="<!-- github-release-metadata:end -->"

usage() {
  cat <<'EOF'
Append GitHub-generated release metadata to a release notes file.

Usage:
  ./scripts/append-github-release-metadata.sh --tag <tag> --notes-file <file> [--previous-tag <tag>] [--output <file>] [--repo <owner/repo>] [--target <ref>]
  ./scripts/append-github-release-metadata.sh --strip-only --notes-file <file> [--output <file>]

Options:
  --tag <tag>            Release tag to generate notes for, for example v0.8.5
  --previous-tag <tag>   Previous release tag used as the generated-notes compare base
  --notes-file <file>    Markdown release notes file to read
  --output <file>        Output path (default: overwrite --notes-file)
  --repo <owner/repo>    GitHub repository (default: GITHUB_REPOSITORY or origin)
  --target <ref>         Target commitish for generated notes (default: --tag)
  --strip-only           Remove a previously appended generated metadata block
  -h, --help             Show this help
EOF
}

die() {
  echo "$*" >&2
  exit 1
}

repo_full_name() {
  if [[ -n "${repo}" ]]; then
    printf '%s\n' "${repo}"
    return
  fi

  if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    printf '%s\n' "${GITHUB_REPOSITORY}"
    return
  fi

  local remote_url
  remote_url="$(git config --get remote.origin.url 2>/dev/null || true)"
  [[ -n "${remote_url}" ]] || die "Could not infer repository. Pass --repo <owner/repo>."

  case "${remote_url}" in
    git@github.com:*)
      remote_url="${remote_url#git@github.com:}"
      ;;
    ssh://git@github.com/*)
      remote_url="${remote_url#ssh://git@github.com/}"
      ;;
    https://github.com/*)
      remote_url="${remote_url#https://github.com/}"
      ;;
    https://*@github.com/*)
      remote_url="${remote_url#https://*@github.com/}"
      ;;
    *)
      die "Could not infer GitHub repository from origin URL. Pass --repo <owner/repo>."
      ;;
  esac

  remote_url="${remote_url%.git}"
  [[ "${remote_url}" =~ ^[^/]+/[^/]+$ ]] || die "Invalid repository inferred from origin URL: ${remote_url}"
  printf '%s\n' "${remote_url}"
}

strip_metadata() {
  local input_file="$1"
  local output_file="$2"

  awk -v start="${START_MARKER}" -v end="${END_MARKER}" '
    $0 == start { dropping = 1; next }
    $0 == end { dropping = 0; next }
    !dropping { print }
  ' "${input_file}" | awk '
    { lines[NR] = $0 }
    END {
      last = NR
      while (last > 0 && lines[last] == "") {
        last--
      }
      for (i = 1; i <= last; i++) {
        print lines[i]
      }
    }
  ' > "${output_file}"
}

tag=""
previous_tag=""
notes_file=""
output_file=""
repo=""
target_commitish=""
strip_only="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --tag)
      shift
      [[ $# -gt 0 ]] || die "--tag requires a value"
      tag="$1"
      ;;
    --previous-tag)
      shift
      [[ $# -gt 0 ]] || die "--previous-tag requires a value"
      previous_tag="$1"
      ;;
    --notes-file)
      shift
      [[ $# -gt 0 ]] || die "--notes-file requires a value"
      notes_file="$1"
      ;;
    --output)
      shift
      [[ $# -gt 0 ]] || die "--output requires a value"
      output_file="$1"
      ;;
    --repo)
      shift
      [[ $# -gt 0 ]] || die "--repo requires a value"
      repo="$1"
      ;;
    --target)
      shift
      [[ $# -gt 0 ]] || die "--target requires a value"
      target_commitish="$1"
      ;;
    --strip-only)
      strip_only="true"
      ;;
    *)
      die "Unknown option: $1"
      ;;
  esac
  shift
done

[[ -n "${notes_file}" ]] || die "--notes-file is required"
[[ -f "${notes_file}" ]] || die "Release notes file not found: ${notes_file}"

if [[ -z "${output_file}" ]]; then
  output_file="${notes_file}"
fi

tmp_stripped="$(mktemp)"
tmp_output="$(mktemp)"
trap 'rm -f "${tmp_stripped}" "${tmp_output}"' EXIT

strip_metadata "${notes_file}" "${tmp_stripped}"

if [[ "${strip_only}" == "true" ]]; then
  cp "${tmp_stripped}" "${tmp_output}"
else
  [[ -n "${tag}" ]] || die "--tag is required unless --strip-only is used"
  [[ -n "${target_commitish}" ]] || target_commitish="${tag}"
  command -v gh >/dev/null 2>&1 || die "gh CLI not found in PATH"

  repo_name="$(repo_full_name)"
  gh_args=(
    -H 'Accept: application/vnd.github+json'
    "repos/${repo_name}/releases/generate-notes"
    -f "tag_name=${tag}"
    -f "target_commitish=${target_commitish}"
    --jq '.body'
  )
  if [[ -n "${previous_tag}" ]]; then
    gh_args+=(-f "previous_tag_name=${previous_tag}")
  fi
  generated_body="$(gh api "${gh_args[@]}")"

  [[ -n "$(printf '%s' "${generated_body}" | tr -d '[:space:]')" ]] || die "Generated release metadata was empty for ${tag}"

  {
    cat "${tmp_stripped}"
    printf '\n\n%s\n' "${START_MARKER}"
    printf '%s\n' "${generated_body}"
    printf '%s\n' "${END_MARKER}"
  } > "${tmp_output}"
fi

mkdir -p "$(dirname "${output_file}")"
mv "${tmp_output}" "${output_file}"
