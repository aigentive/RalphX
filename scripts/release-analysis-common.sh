#!/usr/bin/env bash

RELEASE_ANALYSIS_STATE_DIR=".artifacts/release-notes"
RELEASE_ANALYSIS_LOGS_DIR=".artifacts/release-notes/logs"
RELEASE_ANALYSIS_VERSION_FILE=".artifacts/release-notes/.version"
RELEASE_ANALYSIS_DEVELOPER_INSTRUCTIONS="For this run, do not read CLAUDE.md, DEVELOPMENT.md, or other fallback project docs unless the prompt explicitly names them. Stay within the provided release context, commit bodies, and commit subjects only."

release_analysis_die() {
  echo "$*" >&2
  exit 1
}

release_analysis_validate_reasoning_effort() {
  case "$1" in
    low|medium|high|xhigh)
      ;;
    *)
      release_analysis_die "--reasoning-effort must be low, medium, high, or xhigh"
      ;;
  esac
}

release_analysis_normalize_version() {
  local version="${1#v}"

  [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || release_analysis_die "Invalid semantic version: $1"

  printf '%s\n' "${version}"
}

release_analysis_compare_versions() {
  local left
  local right
  local left_major
  local left_minor
  local left_patch
  local right_major
  local right_minor
  local right_patch

  left="$(release_analysis_normalize_version "$1")"
  right="$(release_analysis_normalize_version "$2")"
  IFS='.' read -r left_major left_minor left_patch <<< "${left}"
  IFS='.' read -r right_major right_minor right_patch <<< "${right}"

  left_major=$((10#${left_major}))
  left_minor=$((10#${left_minor}))
  left_patch=$((10#${left_patch}))
  right_major=$((10#${right_major}))
  right_minor=$((10#${right_minor}))
  right_patch=$((10#${right_patch}))

  if (( left_major < right_major )); then
    printf -- '-1\n'
  elif (( left_major > right_major )); then
    printf '1\n'
  elif (( left_minor < right_minor )); then
    printf -- '-1\n'
  elif (( left_minor > right_minor )); then
    printf '1\n'
  elif (( left_patch < right_patch )); then
    printf -- '-1\n'
  elif (( left_patch > right_patch )); then
    printf '1\n'
  else
    printf '0\n'
  fi
}

release_analysis_assert_version_greater() {
  local current
  local selected
  local comparison

  current="$(release_analysis_normalize_version "$1")"
  selected="$(release_analysis_normalize_version "$2")"
  comparison="$(release_analysis_compare_versions "${current}" "${selected}")"

  [[ "${comparison}" == "-1" ]] || release_analysis_die "Selected release version ${selected} must be greater than current version ${current}."
}

release_analysis_is_major_bump() {
  local current
  local selected
  local current_major
  local selected_major

  current="$(release_analysis_normalize_version "$1")"
  selected="$(release_analysis_normalize_version "$2")"
  IFS='.' read -r current_major _ <<< "${current}"
  IFS='.' read -r selected_major _ <<< "${selected}"

  (( 10#${selected_major} > 10#${current_major} ))
}

release_analysis_assert_major_bump_allowed() {
  local current
  local selected
  local approval_mode="${3:-automatic}"

  current="$(release_analysis_normalize_version "$1")"
  selected="$(release_analysis_normalize_version "$2")"

  if release_analysis_is_major_bump "${current}" "${selected}" && [[ "${approval_mode}" != "manual" ]]; then
    release_analysis_die "Major release bump ${current} -> ${selected} requires explicit manual approval. Re-run Daily Release with release_bump=major or release_version=${selected}, or use a manually approved local release path."
  fi
}

release_analysis_try_read_selected_version() {
  [[ -f "${RELEASE_ANALYSIS_VERSION_FILE}" ]] || return 1

  local version
  version="$(head -n 1 "${RELEASE_ANALYSIS_VERSION_FILE}" | tr -d '[:space:]')"
  [[ -n "${version}" ]] || return 1

  release_analysis_normalize_version "${version}"
}

release_analysis_read_selected_version_or_die() {
  local version
  version="$(release_analysis_try_read_selected_version)" || release_analysis_die "No stored release version found. Run ./scripts/propose-release.sh and accept the proposal, or pass an explicit version."
  printf '%s\n' "${version}"
}

release_analysis_write_selected_version() {
  local version
  version="$(release_analysis_normalize_version "$1")"

  mkdir -p "${RELEASE_ANALYSIS_STATE_DIR}"
  printf '%s\n' "${version}" > "${RELEASE_ANALYSIS_VERSION_FILE}"
}

release_analysis_extract_proposed_version_from_file() {
  local proposal_file="$1"
  [[ -f "${proposal_file}" ]] || return 1

  local version
  version="$(sed -nE 's/^[[:space:]]*-[[:space:]]*Proposed version:[[:space:]]*v?([0-9]+\.[0-9]+\.[0-9]+)[[:space:]]*$/\1/p' "${proposal_file}" | head -n 1)"
  [[ -n "${version}" ]] || return 1

  release_analysis_normalize_version "${version}"
}

release_analysis_resolve_default_from_ref() {
  local target_tag="$1"
  local to_ref_local="$2"

  if git rev-parse -q --verify "refs/tags/${target_tag}" >/dev/null 2>&1; then
    git describe --tags --abbrev=0 "${target_tag}^" 2>/dev/null || true
    return
  fi

  git describe --tags --abbrev=0 "${to_ref_local}" 2>/dev/null || true
}

release_analysis_resolve_range() {
  local release_tag="$1"
  local requested_from_ref="$2"
  local requested_to_ref="$3"
  local from_ref="${requested_from_ref}"

  if [[ -z "${from_ref}" ]]; then
    from_ref="$(release_analysis_resolve_default_from_ref "${release_tag}" "${requested_to_ref}")"
    [[ -n "${from_ref}" ]] || release_analysis_die "No prior tag found. Pass --from <ref> for the first release analysis run."
  fi

  git rev-parse --verify "${from_ref}^{commit}" >/dev/null 2>&1 || release_analysis_die "Invalid --from ref: ${from_ref}"
  git rev-parse --verify "${requested_to_ref}^{commit}" >/dev/null 2>&1 || release_analysis_die "Invalid --to ref: ${requested_to_ref}"

  RELEASE_ANALYSIS_FROM_REF="${from_ref}"
  RELEASE_ANALYSIS_TO_REF="${requested_to_ref}"
  RELEASE_ANALYSIS_RANGE_SPEC="${from_ref}..${requested_to_ref}"
  RELEASE_ANALYSIS_COMMIT_COUNT="$(git rev-list --count "${RELEASE_ANALYSIS_RANGE_SPEC}")"

  [[ "${RELEASE_ANALYSIS_COMMIT_COUNT}" -gt 0 ]] || release_analysis_die "No commits found in range ${RELEASE_ANALYSIS_RANGE_SPEC}"
}

release_analysis_repo_full_name() {
  if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    [[ "${GITHUB_REPOSITORY}" =~ ^[^/]+/[^/]+$ ]] || return 1
    printf '%s\n' "${GITHUB_REPOSITORY}"
    return 0
  fi

  local remote_url
  remote_url="$(git config --get remote.origin.url 2>/dev/null || true)"
  [[ -n "${remote_url}" ]] || return 1

  local repo=""
  case "${remote_url}" in
    git@github.com:*)
      repo="${remote_url#git@github.com:}"
      ;;
    ssh://git@github.com/*)
      repo="${remote_url#ssh://git@github.com/}"
      ;;
    https://github.com/*)
      repo="${remote_url#https://github.com/}"
      ;;
    https://*@github.com/*)
      repo="${remote_url#https://*@github.com/}"
      ;;
    http://github.com/*)
      repo="${remote_url#http://github.com/}"
      ;;
    http://*@github.com/*)
      repo="${remote_url#http://*@github.com/}"
      ;;
    *)
      return 1
      ;;
  esac

  repo="${repo%.git}"
  [[ "${repo}" =~ ^[^/]+/[^/]+$ ]] || return 1

  printf '%s\n' "${repo}"
}

release_analysis_extract_pr_number() {
  local text="$1"
  printf '%s\n' "${text}" | sed -nE 's/.*\(#([0-9]+)\).*/\1/p' | head -n 1
}

release_analysis_fetch_pr_details() {
  local pr_number="$1"

  command -v gh >/dev/null 2>&1 || return 1

  local repo_full_name
  repo_full_name="$(release_analysis_repo_full_name)" || return 1

  GH_PROMPT_DISABLED=1 gh pr view "${pr_number}" \
    --repo "${repo_full_name}" \
    --json title,body,url \
    --template $'URL: {{.url}}\nTitle: {{.title}}\nBody:\n{{.body}}\n' \
    2>/dev/null
}

release_analysis_write_associated_pr_details() {
  local commit_body="$1"
  local pr_number
  pr_number="$(release_analysis_extract_pr_number "${commit_body}" || true)"
  [[ -n "${pr_number}" ]] || return 0

  local pr_details
  pr_details="$(release_analysis_fetch_pr_details "${pr_number}" || true)"
  [[ -n "$(printf '%s' "${pr_details}" | tr -d '[:space:]')" ]] || return 0

  printf 'Associated GitHub PR #%s (primary narrative source when commit body is sparse):\n' "${pr_number}"
  printf '%s\n' "${pr_details}"
}

release_analysis_commit_link() {
  local sha="$1"
  local short_sha="${sha:0:8}"
  local repo_full_name

  if repo_full_name="$(release_analysis_repo_full_name 2>/dev/null)"; then
    printf '[%s](https://github.com/%s/commit/%s)\n' "${short_sha}" "${repo_full_name}" "${sha}"
    return 0
  fi

  printf '%s\n' "${short_sha}"
}

release_analysis_collect_evidence() {
  RELEASE_ANALYSIS_SHORTSTAT="$(git diff --shortstat "${RELEASE_ANALYSIS_RANGE_SPEC}" || true)"
  [[ -n "${RELEASE_ANALYSIS_SHORTSTAT}" ]] || RELEASE_ANALYSIS_SHORTSTAT="No diff stat available."

  RELEASE_ANALYSIS_COMMIT_LOG="$(git log --reverse --no-merges --pretty=format:'- %h %s' "${RELEASE_ANALYSIS_RANGE_SPEC}")"
  RELEASE_ANALYSIS_COMMIT_REFERENCES="$(
    while IFS= read -r sha; do
      [[ -n "${sha}" ]] || continue
      subject="$(git show -s --format=%s "${sha}")"
      printf -- '- %s %s\n' "$(release_analysis_commit_link "${sha}")" "${subject}"
    done < <(git rev-list --reverse "${RELEASE_ANALYSIS_RANGE_SPEC}")
  )"

  RELEASE_ANALYSIS_RAW_COMMIT_BODIES="$(
    while IFS= read -r sha; do
      [[ -n "${sha}" ]] || continue
      body="$(git show -s --format=%B "${sha}")"
      [[ -n "$(printf '%s' "${body}" | tr -d '[:space:]')" ]] || continue
      printf -- '--- %s ---\n' "$(release_analysis_commit_link "${sha}")"
      printf '%s\n\n' "${body}"
      release_analysis_write_associated_pr_details "${body}"
      printf '\n'
    done < <(git rev-list --reverse "${RELEASE_ANALYSIS_RANGE_SPEC}")
  )"
}

release_analysis_write_evidence_sections() {
  local guidance="$1"

  printf -- '- Compare range: %s\n' "${RELEASE_ANALYSIS_RANGE_SPEC}"
  printf -- '- Commit count: %s\n' "${RELEASE_ANALYSIS_COMMIT_COUNT}"
  printf -- '- Diff stat: %s\n' "${RELEASE_ANALYSIS_SHORTSTAT}"
  printf '\nReader guidance:\n%s\n' "${guidance}"
  printf '\nCommit subjects:\n'
  printf '%s\n' "${RELEASE_ANALYSIS_COMMIT_LOG}"
  if [[ -n "${RELEASE_ANALYSIS_COMMIT_REFERENCES}" ]]; then
    printf '\nCommit references (use these exact Markdown links for traceability):\n%s\n' "${RELEASE_ANALYSIS_COMMIT_REFERENCES}"
  fi
  if [[ -n "${RELEASE_ANALYSIS_RAW_COMMIT_BODIES}" ]]; then
    printf '\nRaw commit bodies (primary narrative source):\n%s\n' "${RELEASE_ANALYSIS_RAW_COMMIT_BODIES}"
  fi
}

release_analysis_compute_candidate_versions() {
  local current_version
  local major
  local minor
  local patch

  current_version="$(release_analysis_normalize_version "$1")"
  IFS='.' read -r major minor patch <<< "${current_version}"
  major=$((10#${major}))
  minor=$((10#${minor}))
  patch=$((10#${patch}))

  RELEASE_ANALYSIS_CURRENT_VERSION="${current_version}"
  RELEASE_ANALYSIS_NEXT_PATCH="${major}.${minor}.$((patch + 1))"
  RELEASE_ANALYSIS_NEXT_MINOR="${major}.$((minor + 1)).0"
  RELEASE_ANALYSIS_NEXT_MAJOR="$((major + 1)).0.0"
}

release_analysis_infer_current_version_from_ref() {
  local candidate="${1##refs/tags/}"
  candidate="${candidate#v}"

  [[ "${candidate}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1

  printf '%s\n' "${candidate}"
}

release_analysis_run_codex_with_log() {
  local log_file="$1"
  local model="$2"
  local reasoning_effort="$3"
  shift 3

  local raw_log="${log_file%.log}.raw.log"
  local started_at
  local finished_at
  local session_id
  local exec_events

  started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  if "$@" > "${raw_log}" 2>&1; then
    finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    session_id="$(sed -n 's/^session id: //p' "${raw_log}" | head -n 1)"
    exec_events="$(grep -c '^exec$' "${raw_log}" || true)"

    {
      printf 'status: success\n'
      printf 'started_at: %s\n' "${started_at}"
      printf 'finished_at: %s\n' "${finished_at}"
      printf 'session_id: %s\n' "${session_id:-unknown}"
      printf 'model: %s\n' "${model}"
      printf 'reasoning_effort: %s\n' "${reasoning_effort}"
      printf 'exec_events: %s\n' "${exec_events}"
      printf 'raw_log_retained: false\n'
    } > "${log_file}"

    rm -f "${raw_log}"
    return 0
  fi

  finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  session_id="$(sed -n 's/^session id: //p' "${raw_log}" | head -n 1)"
  exec_events="$(grep -c '^exec$' "${raw_log}" || true)"

  {
    printf 'status: failure\n'
    printf 'started_at: %s\n' "${started_at}"
    printf 'finished_at: %s\n' "${finished_at}"
    printf 'session_id: %s\n' "${session_id:-unknown}"
    printf 'model: %s\n' "${model}"
    printf 'reasoning_effort: %s\n' "${reasoning_effort}"
    printf 'exec_events: %s\n' "${exec_events}"
    printf 'raw_log_retained: true\n'
    printf 'raw_log_path: %s\n' "${raw_log}"
    printf '\nlast_output_tail:\n'
    tail -n 80 "${raw_log}"
  } > "${log_file}"

  echo "Codex invocation failed. Summary: ${log_file}" >&2
  echo "Full raw log: ${raw_log}" >&2
  return 1
}
