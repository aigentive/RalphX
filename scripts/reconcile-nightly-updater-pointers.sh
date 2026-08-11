#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Reconcile fixed Nightly updater pointers for one published prerelease without changing Stable.

Usage:
  ./scripts/reconcile-nightly-updater-pointers.sh \
    --repo <owner/repo> \
    --candidate-tag <vX.Y.Z> \
    --source-dir <path> \
    --pointer-dir <path> \
    --work-dir <path>

The helper accepts only an exact candidate rerun, a complete older Nightly pointer,
or a bounded candidate/prior partial upload. It never mutates a version release or
the Stable pointer release.
EOF
}

repo=""
candidate_tag=""
source_dir=""
pointer_dir=""
work_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --candidate-tag)
      candidate_tag="${2:-}"
      shift 2
      ;;
    --source-dir)
      source_dir="${2:-}"
      shift 2
      ;;
    --pointer-dir)
      pointer_dir="${2:-}"
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
  echo "Nightly updater pointers: $*" >&2
  exit 1
}

assert_tag() {
  [[ "$2" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "$1 must be an exact vX.Y.Z release tag."
}

for required in repo candidate_tag source_dir pointer_dir work_dir; do
  [[ -n "${!required}" ]] || die "--${required//_/-} is required."
done
[[ "${repo}" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] \
  || die "--repo must be an exact owner/repo value."
assert_tag "candidate-tag" "${candidate_tag}"
[[ -d "${source_dir}" && -d "${pointer_dir}" ]] \
  || die "source and pointer directories must exist."
mkdir -p "${work_dir}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

release_json() {
  local tag="$1"
  local json
  json="$(gh release view "${tag}" --repo "${repo}" --json tagName,isDraft,isPrerelease,assets)" \
    || die "Release ${tag} is missing or unreadable."
  jq -e --arg tag "${tag}" '
    (.tagName == $tag)
      and (.isDraft | type == "boolean")
      and (.isPrerelease | type == "boolean")
      and (.assets | type == "array")
  ' <<<"${json}" >/dev/null || die "Release ${tag} has missing or malformed state fields."
  printf '%s\n' "${json}"
}

release_api_json() {
  local tag="$1"
  local json
  json="$(gh api "repos/${repo}/releases/tags/${tag}")" \
    || die "Release API record for ${tag} is missing."
  jq -e 'has("immutable") and (.immutable | type == "boolean")' <<<"${json}" >/dev/null \
    || die "Release API record for ${tag} has no immutable field."
  printf '%s\n' "${json}"
}

source_asset_names() {
  local version="$1"
  printf '%s\n' \
    "RalphX_${version}_aarch64.app.tar.gz" \
    "RalphX_${version}_aarch64.app.tar.gz.sig" \
    "RalphX_${version}_aarch64.dmg" \
    "RalphX_${version}_x86_64.app.tar.gz" \
    "RalphX_${version}_x86_64.app.tar.gz.sig" \
    "RalphX_${version}_x86_64.dmg" \
    "checksums.txt" \
    "latest.json"
}

assert_exact_remote_assets() {
  local tag="$1"
  shift
  local json actual expected
  json="$(release_json "${tag}")"
  actual="$(jq -r '.assets[].name' <<<"${json}" | LC_ALL=C sort)"
  expected="$(printf '%s\n' "$@" | LC_ALL=C sort)"
  [[ "${actual}" == "${expected}" ]] \
    || die "Release ${tag} does not have the required exact asset allowlist."
}

semver_compare() {
  local left="${1#v}"
  local right="${2#v}"
  local left_major left_minor left_patch right_major right_minor right_patch
  IFS=. read -r left_major left_minor left_patch <<<"${left}"
  IFS=. read -r right_major right_minor right_patch <<<"${right}"
  if (( left_major != right_major )); then
    (( left_major < right_major )) && printf '%s\n' -1 || printf '%s\n' 1
  elif (( left_minor != right_minor )); then
    (( left_minor < right_minor )) && printf '%s\n' -1 || printf '%s\n' 1
  elif (( left_patch != right_patch )); then
    (( left_patch < right_patch )) && printf '%s\n' -1 || printf '%s\n' 1
  else
    printf '%s\n' 0
  fi
}

pointer_kind="absent"
pointer_arm_tag=""
pointer_x86_tag=""

pointer_release_json() {
  gh release view updater-nightly --repo "${repo}" --json tagName,isDraft,isPrerelease,assets 2>/dev/null || true
}

assert_pointer_not_latest() {
  local latest_json latest_tag
  latest_json="$(gh api "repos/${repo}/releases/latest" 2>/dev/null || true)"
  [[ -z "${latest_json}" ]] && return
  latest_tag="$(jq -er '.tag_name | select(type == "string")' <<<"${latest_json}")" \
    || die "GitHub latest release API has no tag_name."
  [[ "${latest_tag}" != "updater-nightly" ]] \
    || die "updater-nightly must not be GitHub latest."
}

pointer_manifest_tag() {
  local manifest="$1"
  local tag
  jq -e '
    (.version | type == "string")
      and (.platforms | keys == ["nightly"])
      and (.platforms.nightly.url | type == "string")
      and (.platforms.nightly.signature | type == "string")
  ' "${manifest}" >/dev/null \
    || die "${manifest##*/} is not a valid single-platform Nightly pointer."
  tag="$(jq -r '.version' "${manifest}")"
  assert_tag "Nightly pointer version" "${tag}"
  printf '%s\n' "${tag}"
}

snapshot_pointer_state() {
  local pointer_json actual expected snapshot_dir
  pointer_kind="absent"
  pointer_arm_tag=""
  pointer_x86_tag=""
  pointer_json="$(pointer_release_json)"
  [[ -n "${pointer_json}" ]] || return 0

  jq -e '
    (.tagName == "updater-nightly")
      and (.isDraft | type == "boolean")
      and (.isPrerelease | type == "boolean")
      and (.assets | type == "array")
  ' <<<"${pointer_json}" >/dev/null \
    || die "updater-nightly has missing or malformed pointer-release fields."
  [[ "$(jq -r '.isDraft' <<<"${pointer_json}")" == "false" ]] \
    || die "updater-nightly must not be a draft."
  [[ "$(jq -r '.isPrerelease' <<<"${pointer_json}")" == "true" ]] \
    || die "updater-nightly must be a published prerelease pointer release."
  [[ "$(release_api_json updater-nightly | jq -r '.immutable')" == "false" ]] \
    || die "updater-nightly is immutable."
  assert_pointer_not_latest

  actual="$(jq -r '.assets[].name' <<<"${pointer_json}" | LC_ALL=C sort)"
  expected=$'latest-aarch64.json\nlatest-x86_64.json'
  if [[ -z "${actual}" ]]; then
    pointer_kind="empty"
    return
  fi
  if [[ "${actual}" != "latest-aarch64.json" \
    && "${actual}" != "latest-x86_64.json" \
    && "${actual}" != "${expected}" ]]; then
    die "updater-nightly has a corrupt pointer asset allowlist."
  fi

  snapshot_dir="$(mktemp -d "${work_dir}/pointer-snapshot.XXXXXX")"
  if [[ "${actual}" == "latest-aarch64.json" || "${actual}" == "${expected}" ]]; then
    gh release download updater-nightly --repo "${repo}" --pattern latest-aarch64.json --dir "${snapshot_dir}" >/dev/null
    pointer_arm_tag="$(pointer_manifest_tag "${snapshot_dir}/latest-aarch64.json")"
  fi
  if [[ "${actual}" == "latest-x86_64.json" || "${actual}" == "${expected}" ]]; then
    gh release download updater-nightly --repo "${repo}" --pattern latest-x86_64.json --dir "${snapshot_dir}" >/dev/null
    pointer_x86_tag="$(pointer_manifest_tag "${snapshot_dir}/latest-x86_64.json")"
  fi
  if [[ "${actual}" == "latest-aarch64.json" || "${actual}" == "latest-x86_64.json" ]]; then
    pointer_kind="half"
  elif [[ "${pointer_arm_tag}" == "${pointer_x86_tag}" ]]; then
    pointer_kind="complete"
  else
    pointer_kind="mismatch"
  fi
}

pointer_is_complete_for() {
  [[ "${pointer_kind}" == "complete" && "${pointer_arm_tag}" == "$1" ]]
}

pointer_is_half_for() {
  [[ "${pointer_kind}" == "half" ]] || return 1
  { [[ "${pointer_arm_tag}" == "$1" && -z "${pointer_x86_tag}" ]]; } \
    || { [[ -z "${pointer_arm_tag}" && "${pointer_x86_tag}" == "$1" ]]; }
}

pointer_is_target_and_prior() {
  local target="$1"
  [[ "${pointer_kind}" == "mismatch" ]] || return 1
  { [[ "${pointer_arm_tag}" == "${target}" && -n "${pointer_x86_tag}" ]]; } \
    || { [[ "${pointer_x86_tag}" == "${target}" && -n "${pointer_arm_tag}" ]]; }
}

ensure_pointer_release() {
  if [[ -z "$(pointer_release_json)" ]]; then
    gh release create updater-nightly \
      --repo "${repo}" \
      --title "RalphX Nightly updater pointers" \
      --notes "Nightly Tauri updater pointers; do not download this infrastructure release directly." \
      --prerelease \
      --latest=false
  fi
  snapshot_pointer_state
}

candidate_json="$(release_json "${candidate_tag}")"
[[ "$(jq -r '[.isDraft, .isPrerelease] | @tsv' <<<"${candidate_json}")" == $'false\ttrue' ]] \
  || die "Nightly candidate ${candidate_tag} must be a published prerelease."
[[ "$(release_api_json "${candidate_tag}" | jq -r '.immutable')" == "false" ]] \
  || die "Nightly candidate ${candidate_tag} is immutable."
version="${candidate_tag#v}"
candidate_assets=()
while IFS= read -r asset; do
  candidate_assets+=("${asset}")
done < <(source_asset_names "${version}")
assert_exact_remote_assets "${candidate_tag}" "${candidate_assets[@]}"
bash "${script_dir}/validate-release-promotion.sh" \
  "${candidate_tag}" "${source_dir}" "${pointer_dir}" nightly >/dev/null

remote_source_dir="$(mktemp -d "${work_dir}/source.XXXXXX")"
for asset in "${candidate_assets[@]}"; do
  gh release download "${candidate_tag}" --repo "${repo}" --pattern "${asset}" --dir "${remote_source_dir}" >/dev/null
done
bash "${script_dir}/validate-release-promotion.sh" \
  "${candidate_tag}" "${remote_source_dir}" "${pointer_dir}" nightly >/dev/null

snapshot_pointer_state
case "${pointer_kind}" in
  absent|empty)
    ;;
  complete)
    comparison="$(semver_compare "${candidate_tag}" "${pointer_arm_tag}")"
    [[ "${comparison}" != "-1" ]] \
      || die "Nightly candidate ${candidate_tag} is older than existing pointer ${pointer_arm_tag}."
    ;;
  half)
    pointer_is_half_for "${candidate_tag}" \
      || die "Nightly has a one-asset pointer that does not name candidate ${candidate_tag}."
    ;;
  mismatch)
    pointer_is_target_and_prior "${candidate_tag}" \
      || die "Nightly pointers disagree outside the bounded candidate/prior recovery state."
    prior_tag="${pointer_arm_tag}"
    [[ "${prior_tag}" == "${candidate_tag}" ]] && prior_tag="${pointer_x86_tag}"
    [[ "$(semver_compare "${candidate_tag}" "${prior_tag}")" == "1" ]] \
      || die "Nightly candidate ${candidate_tag} is not newer than partial pointer ${prior_tag}."
    ;;
  *)
    die "Unknown Nightly pointer snapshot."
    ;;
esac

ensure_pointer_release
gh release upload updater-nightly \
  --repo "${repo}" \
  --clobber \
  "${pointer_dir}/latest-aarch64.json" \
  "${pointer_dir}/latest-x86_64.json"
assert_exact_remote_assets updater-nightly latest-aarch64.json latest-x86_64.json

verified_dir="$(mktemp -d "${work_dir}/verified-pointer.XXXXXX")"
gh release download updater-nightly --repo "${repo}" --pattern latest-aarch64.json --dir "${verified_dir}" >/dev/null
gh release download updater-nightly --repo "${repo}" --pattern latest-x86_64.json --dir "${verified_dir}" >/dev/null
bash "${script_dir}/validate-release-promotion.sh" \
  "${candidate_tag}" "${remote_source_dir}" "${verified_dir}" nightly
bash "${script_dir}/verify-public-updater-pointers.sh" \
  "${repo}" updater-nightly "${verified_dir}"

snapshot_pointer_state
pointer_is_complete_for "${candidate_tag}" \
  || die "Nightly pointers did not converge on ${candidate_tag}."

echo "Nightly updater pointers converged on ${candidate_tag}."
