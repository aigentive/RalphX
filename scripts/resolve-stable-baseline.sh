#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Resolve the previous Stable release tag strictly below a ceiling tag.

Usage:
  ./scripts/resolve-stable-baseline.sh --repo <owner/repo> --ceiling-tag <vX.Y.Z>
  ./scripts/resolve-stable-baseline.sh --self-test

Prints the newest published full (non-draft, non-prerelease) vX.Y.Z release tag that is
strictly older than --ceiling-tag, or an empty line when no such release exists.

This resolver only identifies a tag name for a git range. Unlike the promote/halt state
machine it deliberately does NOT assert that the resolved release is mutable: an old
immutable Stable release is a perfectly valid notes baseline.

Exit codes:
  0  A prior Stable tag was printed, or no prior Stable release exists (empty line)
  1  Invalid arguments, network failure, or malformed releases API response
EOF
}

repo=""
ceiling_tag=""
self_test="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --ceiling-tag)
      ceiling_tag="${2:-}"
      shift 2
      ;;
    --self-test)
      self_test="true"
      shift
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
  echo "Stable baseline resolver: $*" >&2
  exit 1
}

assert_tag() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "${label} must be an exact vX.Y.Z release tag."
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

# Selects the newest full-release vX.Y.Z tag strictly below the ceiling from a
# paginated (slurped) GitHub releases JSON payload.
select_prior_full_tag() {
  local releases="$1"
  local ceiling="$2"
  local tag best=""
  jq -e 'type == "array" and all(.[]; type == "array")' <<<"${releases}" >/dev/null \
    || die "Release pagination has malformed fields."
  while IFS= read -r tag; do
    [[ -z "${tag}" ]] && continue
    [[ "$(semver_compare "${tag}" "${ceiling}")" == "-1" ]] || continue
    if [[ -z "${best}" || "$(semver_compare "${tag}" "${best}")" == "1" ]]; then
      best="${tag}"
    fi
  done < <(jq -r '.[] | .[] | select(.draft == false and .prerelease == false) | .tag_name | select(test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))' <<<"${releases}")
  printf '%s\n' "${best}"
}

run_self_test() {
  local failures=0
  local payload actual

  check() {
    local label="$1"
    local expected="$2"
    local got="$3"
    if [[ "${got}" == "${expected}" ]]; then
      echo "ok   ${label}"
    else
      echo "FAIL ${label}: expected '${expected}', got '${got}'" >&2
      failures=$((failures + 1))
    fi
  }

  payload='[[
    {"tag_name":"v0.90.0","draft":false,"prerelease":true},
    {"tag_name":"v0.89.0","draft":false,"prerelease":false},
    {"tag_name":"v0.88.0","draft":false,"prerelease":false},
    {"tag_name":"updater-stable","draft":false,"prerelease":true}
  ]]'
  actual="$(select_prior_full_tag "${payload}" v0.90.0)"
  check "picks newest full release below ceiling" "v0.89.0" "${actual}"

  actual="$(select_prior_full_tag "${payload}" v0.89.0)"
  check "excludes the ceiling itself" "v0.88.0" "${actual}"

  payload='[[
    {"tag_name":"v0.88.0","draft":false,"prerelease":false},
    {"tag_name":"v0.9.0","draft":false,"prerelease":false},
    {"tag_name":"v0.10.0","draft":false,"prerelease":false}
  ]]'
  actual="$(select_prior_full_tag "${payload}" v0.90.0)"
  check "compares numerically, not lexically" "v0.88.0" "${actual}"

  payload='[[
    {"tag_name":"v0.89.0","draft":true,"prerelease":false},
    {"tag_name":"v0.88.0","draft":false,"prerelease":true},
    {"tag_name":"nightly","draft":false,"prerelease":false},
    {"tag_name":"v0.87.0-rc1","draft":false,"prerelease":false}
  ]]'
  actual="$(select_prior_full_tag "${payload}" v0.90.0)"
  check "rejects drafts, prereleases, and non-vX.Y.Z tags" "" "${actual}"

  payload='[[{"tag_name":"v0.91.0","draft":false,"prerelease":false}]]'
  actual="$(select_prior_full_tag "${payload}" v0.90.0)"
  check "empty when only newer full releases exist" "" "${actual}"

  payload='[
    [{"tag_name":"v0.89.0","draft":false,"prerelease":false}],
    [{"tag_name":"v0.85.0","draft":false,"prerelease":false}]
  ]'
  actual="$(select_prior_full_tag "${payload}" v0.90.0)"
  check "spans multiple pagination pages" "v0.89.0" "${actual}"

  if (select_prior_full_tag '{"not":"an array of pages"}' v0.90.0) >/dev/null 2>&1; then
    echo "FAIL malformed payload should exit non-zero" >&2
    failures=$((failures + 1))
  else
    echo "ok   rejects malformed pagination payload"
  fi

  if (( failures > 0 )); then
    echo "${failures} self-test failure(s)." >&2
    exit 1
  fi
  echo "All Stable baseline resolver self-tests passed."
}

if [[ "${self_test}" == "true" ]]; then
  [[ -z "${repo}" && -z "${ceiling_tag}" ]] \
    || die "--self-test does not accept --repo or --ceiling-tag."
  run_self_test
  exit 0
fi

[[ -n "${repo}" ]] || die "--repo is required."
[[ "${repo}" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] \
  || die "--repo must be an exact owner/repo value."
[[ -n "${ceiling_tag}" ]] || die "--ceiling-tag is required."
assert_tag "--ceiling-tag" "${ceiling_tag}"

command -v gh >/dev/null 2>&1 || die "gh CLI not found in PATH."
command -v jq >/dev/null 2>&1 || die "jq not found in PATH."

releases="$(gh api --paginate --slurp "repos/${repo}/releases?per_page=100")" \
  || die "Cannot page releases to derive the prior Stable release."

select_prior_full_tag "${releases}" "${ceiling_tag}"
