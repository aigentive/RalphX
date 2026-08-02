#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Reconcile the GitHub, fixed Stable updater-pointer, and Homebrew prerequisites without building or tagging.

Usage:
  ./scripts/reconcile-stable-release-state.sh \
    --operation <promote|halt> \
    --repo <owner/repo> \
    --work-dir <path> \
    --output <path> \
    [--candidate-tag <vX.Y.Z>] \
    [--bad-tag <vX.Y.Z> --restore-tag <vX.Y.Z>] \
    [--promote-body-notes-file <path> --pointer-notes-file <path>]

The ordered mutation contract is: validate/download/render/stage the target cask, pre-create or
validate Stable pointer infrastructure, reconcile versioned GitHub authority, apply any requested
combined-notes presentation, then publish both Stable pointers. The workflow reconciles the
already-staged Homebrew cask only after convergence.
It never builds, tags, pushes, or mutates a version release other than the requested candidate/bad/restore.

Promote-only notes overrides (both or neither):
  --promote-body-notes-file  Markdown written to the promoted release body
  --pointer-notes-file       Markdown written into the promoted release's latest.json and both
                             Stable updater pointers
EOF
}

operation=""
candidate_tag=""
bad_tag=""
restore_tag=""
repo=""
work_dir=""
output_file=""
promote_body_notes_file=""
pointer_notes_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --operation)
      operation="${2:-}"
      shift 2
      ;;
    --candidate-tag)
      candidate_tag="${2:-}"
      shift 2
      ;;
    --bad-tag)
      bad_tag="${2:-}"
      shift 2
      ;;
    --restore-tag)
      restore_tag="${2:-}"
      shift 2
      ;;
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --work-dir)
      work_dir="${2:-}"
      shift 2
      ;;
    --output)
      output_file="${2:-}"
      shift 2
      ;;
    --promote-body-notes-file)
      promote_body_notes_file="${2:-}"
      shift 2
      ;;
    --pointer-notes-file)
      pointer_notes_file="${2:-}"
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
  echo "Stable release control: $*" >&2
  exit 1
}

assert_tag() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "${label} must be an exact vX.Y.Z release tag."
}

for required in repo work_dir output_file; do
  [[ -n "${!required}" ]] || die "--${required//_/-} is required."
done
[[ "${repo}" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] \
  || die "--repo must be an exact owner/repo value."

assert_readable_notes_file() {
  local label="$1"
  local path="$2"
  [[ -f "${path}" && -r "${path}" ]] \
    || die "${label} must be a readable file: ${path}"
  [[ -s "${path}" ]] \
    || die "${label} must not be empty: ${path}"
}

case "${operation}" in
  promote)
    assert_tag "candidate-tag" "${candidate_tag}"
    [[ -z "${bad_tag}" && -z "${restore_tag}" ]] \
      || die "promote accepts only --candidate-tag."
    if [[ -n "${promote_body_notes_file}" || -n "${pointer_notes_file}" ]]; then
      [[ -n "${promote_body_notes_file}" && -n "${pointer_notes_file}" ]] \
        || die "--promote-body-notes-file and --pointer-notes-file must be supplied together."
      assert_readable_notes_file "--promote-body-notes-file" "${promote_body_notes_file}"
      assert_readable_notes_file "--pointer-notes-file" "${pointer_notes_file}"
    fi
    ;;
  halt)
    [[ -z "${candidate_tag}" ]] || die "halt does not accept --candidate-tag."
    [[ -z "${promote_body_notes_file}" && -z "${pointer_notes_file}" ]] \
      || die "halt does not accept combined-notes overrides."
    assert_tag "bad-tag" "${bad_tag}"
    assert_tag "restore-tag" "${restore_tag}"
    ;;
  *)
    die "--operation must be promote or halt."
    ;;
esac

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

assert_mutable_published_release() {
  local tag="$1"
  local release api
  release="$(release_json "${tag}")"
  api="$(release_api_json "${tag}")"
  [[ "$(jq -r '.isDraft' <<<"${release}")" == "false" ]] \
    || die "Draft release ${tag} cannot enter Stable control."
  [[ "$(jq -r '.immutable' <<<"${api}")" == "false" ]] \
    || die "Immutable release ${tag} cannot be changed by Stable control."
}

release_state() {
  local tag="$1"
  release_json "${tag}" | jq -r '[.isDraft, .isPrerelease] | @tsv'
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

assert_newer_than() {
  local candidate="$1"
  local baseline="$2"
  [[ "$(semver_compare "${candidate}" "${baseline}")" == "1" ]] \
    || die "${candidate} must be newer than Stable baseline ${baseline}."
}

github_latest_full_tag() {
  local latest_json tag
  if ! latest_json="$(gh api "repos/${repo}/releases/latest" 2>/dev/null)"; then
    printf '\n'
    return
  fi
  jq -e '
    (.tag_name | type == "string")
      and (.draft == false)
      and (.prerelease == false)
  ' <<<"${latest_json}" >/dev/null \
    || die "GitHub latest release is missing required full-release fields."
  tag="$(jq -r '.tag_name' <<<"${latest_json}")"
  assert_tag "GitHub latest release" "${tag}"
  printf '%s\n' "${tag}"
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

download_source_assets() {
  local tag="$1"
  local version="${tag#v}"
  local destination asset
  destination="$(mktemp -d "${work_dir}/source.XXXXXX")"
  assert_exact_remote_assets "${tag}" \
    "RalphX_${version}_aarch64.app.tar.gz" \
    "RalphX_${version}_aarch64.app.tar.gz.sig" \
    "RalphX_${version}_aarch64.dmg" \
    "RalphX_${version}_x86_64.app.tar.gz" \
    "RalphX_${version}_x86_64.app.tar.gz.sig" \
    "RalphX_${version}_x86_64.dmg" \
    "checksums.txt" \
    "latest.json"
  while IFS= read -r asset; do
    gh release download "${tag}" --repo "${repo}" --pattern "${asset}" --dir "${destination}" >/dev/null
  done < <(source_asset_names "${version}")
  printf '%s\n' "${destination}"
}

staged_latest_json_dir="${work_dir}/staged-latest-json"

render_and_validate_pointer() {
  local tag="$1"
  local source_dir="$2"
  local pointer_dir="$3"
  local notes_override_file="${4:-}"
  local notes_file render_dir pub_date
  notes_file="$(mktemp "${work_dir}/notes.XXXXXX")"
  render_dir="$(mktemp -d "${work_dir}/render.XXXXXX")"
  mkdir -p "${pointer_dir}"
  jq -e '(.notes | type == "string") and (.pub_date | type == "string")' \
    "${source_dir}/latest.json" >/dev/null \
    || die "${tag} latest.json is missing updater notes or pub_date."
  if [[ -n "${notes_override_file}" ]]; then
    assert_readable_notes_file "combined pointer notes for ${tag}" "${notes_override_file}"
    cp "${notes_override_file}" "${notes_file}"
  else
    jq -r '.notes' "${source_dir}/latest.json" >"${notes_file}"
  fi
  pub_date="$(jq -r '.pub_date' "${source_dir}/latest.json")"
  bash "${script_dir}/render-updater-channel-manifests.sh" \
    --tag "${tag}" \
    --notes-file "${notes_file}" \
    --aarch64-signature "${source_dir}/RalphX_${tag#v}_aarch64.app.tar.gz.sig" \
    --x86_64-signature "${source_dir}/RalphX_${tag#v}_x86_64.app.tar.gz.sig" \
    --pub-date "${pub_date}" \
    --channel stable \
    --output-dir "${render_dir}"
  cp "${render_dir}/latest-aarch64.json" "${pointer_dir}/latest-aarch64.json"
  cp "${render_dir}/latest-x86_64.json" "${pointer_dir}/latest-x86_64.json"
  # Only a notes-override render may replace the released versioned manifest; every other
  # call path leaves the source latest.json exactly as published.
  if [[ -n "${notes_override_file}" ]]; then
    mkdir -p "${staged_latest_json_dir}"
    cp "${render_dir}/latest.json" "${staged_latest_json_dir}/latest.json"
  fi
  bash "${script_dir}/validate-release-promotion.sh" \
    "${tag}" "${source_dir}" "${pointer_dir}" stable >/dev/null
}

stage_target_homebrew_cask() {
  local tag="$1"
  local source_dir="$2"
  local version arm_sha intel_sha staged_cask
  version="${tag#v}"
  staged_cask="${work_dir}/staged-homebrew-cask.rb"
  arm_sha="$(shasum -a 256 "${source_dir}/RalphX_${version}_aarch64.dmg" | awk '{print $1}')"
  intel_sha="$(shasum -a 256 "${source_dir}/RalphX_${version}_x86_64.dmg" | awk '{print $1}')"
  [[ "${arm_sha}" =~ ^[0-9a-f]{64}$ && "${intel_sha}" =~ ^[0-9a-f]{64}$ ]] \
    || die "Could not calculate deterministic DMG checksums for ${tag}."
  bash "${script_dir}/render-homebrew-cask.sh" "${version}" "${arm_sha}" "${intel_sha}" >"${staged_cask}"
  [[ -s "${staged_cask}" ]] || die "Could not stage the target Homebrew cask for ${tag}."
}

prepare_pointer_for_tag() {
  local tag="$1"
  local source_dir pointer_dir
  source_dir="$(download_source_assets "${tag}")"
  pointer_dir="$(mktemp -d "${work_dir}/expected-pointer.XXXXXX")"
  render_and_validate_pointer "${tag}" "${source_dir}" "${pointer_dir}"
  printf '%s\t%s\n' "${source_dir}" "${pointer_dir}"
}

prepare_stable_target_for_tag() {
  local tag="$1"
  local notes_override_file="${2:-}"
  local source_dir pointer_dir
  source_dir="$(download_source_assets "${tag}")"
  pointer_dir="$(mktemp -d "${work_dir}/expected-pointer.XXXXXX")"
  render_and_validate_pointer "${tag}" "${source_dir}" "${pointer_dir}" "${notes_override_file}"
  stage_target_homebrew_cask "${tag}" "${source_dir}"
  printf '%s\t%s\n' "${source_dir}" "${pointer_dir}"
}

pointer_release_json() {
  gh release view updater-stable --repo "${repo}" --json tagName,isDraft,isPrerelease,assets 2>/dev/null || true
}

pointer_kind="absent"
pointer_arm_tag=""
pointer_x86_tag=""

pointer_manifest_tag() {
  local manifest="$1"
  local tag
  jq -e '
    (.version | type == "string")
      and (.platforms | keys == ["stable"])
      and (.platforms.stable.url | type == "string")
      and (.platforms.stable.signature | type == "string")
  ' "${manifest}" >/dev/null \
    || die "${manifest##*/} is not a valid single-platform Stable pointer."
  tag="$(jq -r '.version' "${manifest}")"
  assert_tag "Stable pointer version" "${tag}"
  printf '%s\n' "${tag}"
}

assert_pointer_not_github_latest() {
  local latest_tag
  latest_tag="$(github_latest_full_tag)"
  [[ "${latest_tag}" != "updater-stable" ]] \
    || die "updater-stable must not be GitHub latest."
}

snapshot_pointer_state() {
  local pointer_json actual expected snapshot_dir
  pointer_kind="absent"
  pointer_arm_tag=""
  pointer_x86_tag=""
  pointer_json="$(pointer_release_json)"
  [[ -n "${pointer_json}" ]] || return 0

  jq -e '
    (.tagName == "updater-stable")
      and (.isDraft | type == "boolean")
      and (.isPrerelease | type == "boolean")
      and (.assets | type == "array")
  ' <<<"${pointer_json}" >/dev/null \
    || die "updater-stable has missing or malformed pointer-release fields."
  [[ "$(jq -r '.isDraft' <<<"${pointer_json}")" == "false" ]] \
    || die "updater-stable must not be a draft."
  [[ "$(jq -r '.isPrerelease' <<<"${pointer_json}")" == "true" ]] \
    || die "updater-stable must be a published prerelease pointer release."
  [[ "$(release_api_json updater-stable | jq -r '.immutable')" == "false" ]] \
    || die "updater-stable is immutable."
  assert_pointer_not_github_latest

  actual="$(jq -r '.assets[].name' <<<"${pointer_json}" | LC_ALL=C sort)"
  expected=$'latest-aarch64.json\nlatest-x86_64.json'
  if [[ -z "${actual}" ]]; then
    pointer_kind="empty"
    return
  fi
  if [[ "${actual}" != "latest-aarch64.json" \
    && "${actual}" != "latest-x86_64.json" \
    && "${actual}" != "${expected}" ]]; then
    die "updater-stable has a corrupt pointer asset allowlist."
  fi

  snapshot_dir="$(mktemp -d "${work_dir}/pointer-snapshot.XXXXXX")"
  if [[ "${actual}" == "latest-aarch64.json" || "${actual}" == "${expected}" ]]; then
    gh release download updater-stable --repo "${repo}" --pattern latest-aarch64.json --dir "${snapshot_dir}" >/dev/null
    pointer_arm_tag="$(pointer_manifest_tag "${snapshot_dir}/latest-aarch64.json")"
  fi
  if [[ "${actual}" == "latest-x86_64.json" || "${actual}" == "${expected}" ]]; then
    gh release download updater-stable --repo "${repo}" --pattern latest-x86_64.json --dir "${snapshot_dir}" >/dev/null
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
  local tag="$1"
  [[ "${pointer_kind}" == "complete" && "${pointer_arm_tag}" == "${tag}" ]]
}

pointer_is_half_for() {
  local tag="$1"
  [[ "${pointer_kind}" == "half" ]] || return 1
  { [[ "${pointer_arm_tag}" == "${tag}" && -z "${pointer_x86_tag}" ]]; } \
    || { [[ -z "${pointer_arm_tag}" && "${pointer_x86_tag}" == "${tag}" ]]; }
}

pointer_is_target_and_prior() {
  local target="$1"
  local prior="$2"
  [[ "${pointer_kind}" == "mismatch" ]] || return 1
  { [[ "${pointer_arm_tag}" == "${target}" && "${pointer_x86_tag}" == "${prior}" ]]; } \
    || { [[ "${pointer_arm_tag}" == "${prior}" && "${pointer_x86_tag}" == "${target}" ]]; }
}

derive_prior_full_tag() {
  local ceiling="$1"
  local releases tag best=""
  releases="$(gh api --paginate --slurp "repos/${repo}/releases?per_page=100")" \
    || die "Cannot page releases to derive the prior Stable release."
  jq -e 'type == "array" and all(.[]; type == "array")' <<<"${releases}" >/dev/null \
    || die "Release pagination has malformed fields."
  while IFS= read -r tag; do
    [[ -z "${tag}" ]] && continue
    [[ "$(semver_compare "${tag}" "${ceiling}")" == "-1" ]] || continue
    if [[ -z "${best}" || "$(semver_compare "${tag}" "${best}")" == "1" ]]; then
      best="${tag}"
    fi
  done < <(jq -r '.[] | .[] | select(.draft == false and .prerelease == false) | .tag_name | select(test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))' <<<"${releases}")
  [[ -n "${best}" ]] || {
    printf '\n'
    return
  }
  assert_mutable_published_release "${best}"
  printf '%s\n' "${best}"
}

ensure_pointer_release() {
  local pointer_json
  pointer_json="$(pointer_release_json)"
  if [[ -z "${pointer_json}" ]]; then
    gh release create updater-stable \
      --repo "${repo}" \
      --title "RalphX Stable updater pointers" \
      --notes "Stable Tauri updater pointers; do not download this infrastructure release directly." \
      --prerelease \
      --latest=false
  fi
  [[ "$(release_state updater-stable)" == $'false\ttrue' ]] \
    || die "updater-stable did not reach published prerelease/latest=false state."
  assert_mutable_published_release updater-stable
  assert_pointer_not_github_latest
}

publish_and_verify_pointer() {
  local tag="$1"
  local source_dir="$2"
  local pointer_dir="$3"
  local verified_dir
  [[ "$(release_state updater-stable)" == $'false\ttrue' ]] \
    || die "updater-stable must be pre-created as a published prerelease before pointer publication."
  assert_mutable_published_release updater-stable
  assert_pointer_not_github_latest
  gh release upload updater-stable \
    --repo "${repo}" \
    --clobber \
    "${pointer_dir}/latest-aarch64.json" \
    "${pointer_dir}/latest-x86_64.json"
  assert_exact_remote_assets updater-stable latest-aarch64.json latest-x86_64.json
  verified_dir="$(mktemp -d "${work_dir}/verified-pointer.XXXXXX")"
  gh release download updater-stable --repo "${repo}" --pattern latest-aarch64.json --dir "${verified_dir}" >/dev/null
  gh release download updater-stable --repo "${repo}" --pattern latest-x86_64.json --dir "${verified_dir}" >/dev/null
  bash "${script_dir}/validate-release-promotion.sh" \
    "${tag}" "${source_dir}" "${verified_dir}" stable
  bash "${script_dir}/verify-public-updater-pointers.sh" \
    "${repo}" updater-stable "${verified_dir}"
}

set_promotion_github_authority() {
  local tag="$1"
  if [[ "$(release_state "${tag}")" != $'false\tfalse' \
    || "$(github_latest_full_tag)" != "${tag}" ]]; then
    gh release edit "${tag}" --repo "${repo}" --prerelease=false --latest=true
  fi
  [[ "$(release_state "${tag}")" == $'false\tfalse' ]] \
    || die "${tag} did not reach Stable full/latest state."
  [[ "$(github_latest_full_tag)" == "${tag}" ]] \
    || die "GitHub latest did not converge on ${tag}."
}

apply_combined_notes_presentation() {
  local tag="$1"
  local version="${tag#v}"
  local staged_latest_json="${staged_latest_json_dir}/latest.json"
  local verified_dir
  assert_readable_notes_file "combined release body notes" "${promote_body_notes_file}"
  [[ -s "${staged_latest_json}" ]] \
    || die "Combined-notes promotion has no staged latest.json for ${tag}."
  assert_mutable_published_release "${tag}"

  gh release edit "${tag}" --repo "${repo}" --notes-file "${promote_body_notes_file}"
  gh release upload "${tag}" --repo "${repo}" --clobber "${staged_latest_json}"

  # The asset name set is unchanged; latest.json was replaced in place.
  assert_exact_remote_assets "${tag}" \
    "RalphX_${version}_aarch64.app.tar.gz" \
    "RalphX_${version}_aarch64.app.tar.gz.sig" \
    "RalphX_${version}_aarch64.dmg" \
    "RalphX_${version}_x86_64.app.tar.gz" \
    "RalphX_${version}_x86_64.app.tar.gz.sig" \
    "RalphX_${version}_x86_64.dmg" \
    "checksums.txt" \
    "latest.json"

  verified_dir="$(mktemp -d "${work_dir}/verified-latest-json.XXXXXX")"
  gh release download "${tag}" --repo "${repo}" --pattern latest.json --dir "${verified_dir}" >/dev/null
  cmp -s "${verified_dir}/latest.json" "${staged_latest_json}" \
    || die "Published ${tag} latest.json does not byte-match the staged combined-notes manifest."
}

set_halt_github_authority() {
  local bad="$1"
  local restore="$2"
  if [[ "$(release_state "${bad}")" != $'false\ttrue' ]]; then
    gh release edit "${bad}" --repo "${repo}" --prerelease --latest=false
  fi
  if [[ "$(release_state "${restore}")" != $'false\tfalse' \
    || "$(github_latest_full_tag)" != "${restore}" ]]; then
    gh release edit "${restore}" --repo "${repo}" --prerelease=false --latest=true
  fi
  [[ "$(release_state "${bad}")" == $'false\ttrue' ]] \
    || die "Bad Stable release ${bad} did not demote cleanly."
  [[ "$(release_state "${restore}")" == $'false\tfalse' ]] \
    || die "Restore release ${restore} did not become GitHub latest."
  [[ "$(github_latest_full_tag)" == "${restore}" ]] \
    || die "GitHub latest did not converge on restore release ${restore}."
}

selected_tag=""

if [[ "${operation}" == "promote" ]]; then
  assert_mutable_published_release "${candidate_tag}"
  IFS=$'\t' read -r source_dir pointer_dir \
    < <(prepare_stable_target_for_tag "${candidate_tag}" "${pointer_notes_file}")
  candidate_state="$(release_state "${candidate_tag}")"
  latest_tag="$(github_latest_full_tag)"
  prior_tag="$(derive_prior_full_tag "${candidate_tag}")"
  snapshot_pointer_state

  if [[ "${candidate_state}" == $'false\tfalse' && "${latest_tag}" == "${candidate_tag}" ]]; then
    case "${pointer_kind}" in
      absent|empty)
        ;;
      complete)
        [[ "${pointer_arm_tag}" == "${candidate_tag}" \
          || ( -n "${prior_tag}" && "${pointer_arm_tag}" == "${prior_tag}" ) ]] \
          || die "GitHub already advanced to ${candidate_tag}, but Stable pointers name an unrelated release."
        ;;
      half)
        pointer_is_half_for "${candidate_tag}" \
          || die "GitHub already advanced to ${candidate_tag}, but Stable has an unrelated half pointer."
        ;;
      mismatch)
        pointer_is_target_and_prior "${candidate_tag}" "${prior_tag}" \
          || die "GitHub already advanced to ${candidate_tag}, but Stable pointers disagree outside the bounded recovery state."
        ;;
      *)
        die "Unknown Stable pointer snapshot."
        ;;
    esac
  elif [[ "${candidate_state}" == $'false\ttrue' ]]; then
    if [[ -z "${latest_tag}" ]]; then
      [[ "${pointer_kind}" == "absent" || "${pointer_kind}" == "empty" ]] \
        || die "First Stable promotion requires absent or empty Stable pointers."
    else
      assert_newer_than "${candidate_tag}" "${latest_tag}"
      if [[ "${pointer_kind}" == "absent" || "${pointer_kind}" == "empty" ]]; then
        : # First Stable-pointer promotion can adopt an existing full GitHub baseline.
      else
        pointer_is_complete_for "${latest_tag}" \
          || die "New Stable promotion requires both Stable pointers to prove GitHub latest ${latest_tag}."
      fi
    fi
  else
    die "Promotion requires ${candidate_tag} to be a published prerelease or the exact current Stable release."
  fi

  # Phase 2: the absent pointer release must become a mutable prerelease before
  # version authority changes, so infrastructure creation cannot leave GitHub latest advanced.
  ensure_pointer_release
  # Phase 3: GitHub authority. Older full releases remain full history.
  set_promotion_github_authority "${candidate_tag}"
  # Phase 3.5: presentation only. Both operations are clobber-style, so an interrupted run
  # that reaches here reapplies them idempotently on the next exact rerun.
  if [[ -n "${pointer_notes_file}" ]]; then
    apply_combined_notes_presentation "${candidate_tag}"
  fi
  # Phase 4: only after GitHub latest is proven, publish both Stable pointers.
  publish_and_verify_pointer "${candidate_tag}" "${source_dir}" "${pointer_dir}"
  snapshot_pointer_state
  pointer_is_complete_for "${candidate_tag}" \
    || die "Stable pointers did not converge on ${candidate_tag}."
  selected_tag="${candidate_tag}"
else
  assert_mutable_published_release "${bad_tag}"
  derived_restore_tag="$(derive_prior_full_tag "${bad_tag}")"
  [[ -n "${derived_restore_tag}" ]] \
    || die "No previously promoted full release exists below ${bad_tag}."
  [[ "${restore_tag}" == "${derived_restore_tag}" ]] \
    || die "restore-tag ${restore_tag} does not equal derived prior Stable release ${derived_restore_tag}."
  assert_mutable_published_release "${restore_tag}"
  # Validate both the bad current source and the derived restore before any mutation.
  prepare_pointer_for_tag "${bad_tag}" >/dev/null
  IFS=$'\t' read -r source_dir pointer_dir < <(prepare_stable_target_for_tag "${restore_tag}")
  bad_state="$(release_state "${bad_tag}")"
  restore_state="$(release_state "${restore_tag}")"
  latest_tag="$(github_latest_full_tag)"
  snapshot_pointer_state

  if [[ "${bad_state}" == $'false\tfalse' \
    && "${restore_state}" == $'false\tfalse' \
    && "${latest_tag}" == "${bad_tag}" ]]; then
    pointer_is_complete_for "${bad_tag}" \
      || die "halt requires both Stable pointers to prove the current bad-tag ${bad_tag}."
  elif [[ "${bad_state}" == $'false\ttrue' \
    && "${restore_state}" == $'false\tfalse' \
    && "${latest_tag}" == "${restore_tag}" ]]; then
    if pointer_is_complete_for "${bad_tag}" || pointer_is_complete_for "${restore_tag}"; then
      : # Before or after the two-pointer publish phase.
    elif pointer_is_target_and_prior "${restore_tag}" "${bad_tag}"; then
      : # Exactly one architecture reached the requested restore before interruption.
    else
      die "halt found Stable pointers outside the bounded bad-tag/restore-tag recovery state."
    fi
  else
    die "halt requires the exact current bad-tag state or a proven exact rerun after GitHub authority advanced."
  fi

  # Phase 2: verify the pointer infrastructure before GitHub authority changes.
  ensure_pointer_release
  # Phase 3: demote bad-tag and promote restore-tag before changing updater pointers.
  set_halt_github_authority "${bad_tag}" "${restore_tag}"
  # Phase 4: restore both pointers; the workflow updates Homebrew only after this returns success.
  publish_and_verify_pointer "${restore_tag}" "${source_dir}" "${pointer_dir}"
  snapshot_pointer_state
  pointer_is_complete_for "${restore_tag}" \
    || die "Stable pointers did not converge on restore release ${restore_tag}."
  selected_tag="${restore_tag}"
fi

{
  printf 'selected_tag=%s\n' "${selected_tag}"
  printf 'operation=%s\n' "${operation}"
} >"${output_file}"

echo "Stable ${operation} GitHub and pointer authorities converged on ${selected_tag}."
