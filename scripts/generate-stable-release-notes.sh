#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROMPT_FILE="${SCRIPT_DIR}/prompts/stable-release-notes-codex-prompt.md"
COMMON_FILE="${SCRIPT_DIR}/release-analysis-common.sh"
METADATA_SCRIPT="${SCRIPT_DIR}/append-github-release-metadata.sh"
DEFAULT_MODEL="${STABLE_RELEASE_NOTES_MODEL:-gpt-5.6-terra}"
DEFAULT_REASONING_EFFORT="${STABLE_RELEASE_NOTES_REASONING_EFFORT:-xhigh}"

usage() {
  cat <<'EOF'
Combine the committed per-build release notes since the previous Stable release into one
cumulative Stable release note with Codex CLI.

Usage:
  ./scripts/generate-stable-release-notes.sh \
    --candidate-tag vX.Y.Z --from-stable-tag vA.B.C \
    [--model <model>] [--reasoning-effort <low|medium|high|xhigh>] \
    [--output <file>] [--context-only]

Options:
  --candidate-tag <vX.Y.Z>   Prerelease tag being promoted to Stable (range ceiling, inclusive)
  --from-stable-tag <vA.B.C> Previous Stable release tag (range floor, exclusive)
  --model <model>            Codex model to use (default: STABLE_RELEASE_NOTES_MODEL or gpt-5.6-terra)
  --reasoning-effort <level> Codex reasoning effort (default: STABLE_RELEASE_NOTES_REASONING_EFFORT or xhigh)
  --output <file>            Output markdown path
                             (default: .artifacts/release-notes/stable-<candidate>.md,
                              or .artifacts/release-notes/stable-context-<candidate>.md with --context-only)
  --context-only             Write the assembled combine context instead of invoking Codex
  -h, --help                 Show this help

Notes:
  - Per-build notes are read from the candidate tag's committed tree (release-notes/vX.Y.Z.md),
    so no extra checkout is required.
  - Version tags with no committed notes file are skipped with a warning; the next tag's notes
    normally already cover those commits.
  - This script does not append the GitHub metadata block; run
    ./scripts/append-github-release-metadata.sh afterwards, as Daily Release does.
EOF
}

# shellcheck source=scripts/release-analysis-common.sh
# shellcheck disable=SC1091
source "${COMMON_FILE}"

candidate_tag=""
from_stable_tag=""
model="${DEFAULT_MODEL}"
reasoning_effort="${DEFAULT_REASONING_EFFORT}"
output_path=""
context_only="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --candidate-tag)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--candidate-tag requires a vX.Y.Z tag"
      candidate_tag="$1"
      ;;
    --from-stable-tag)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--from-stable-tag requires a vX.Y.Z tag"
      from_stable_tag="$1"
      ;;
    --model)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--model requires a model name"
      model="$1"
      ;;
    --reasoning-effort)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--reasoning-effort requires low, medium, high, or xhigh"
      reasoning_effort="$1"
      ;;
    --output)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--output requires a path"
      output_path="$1"
      ;;
    --context-only)
      context_only="true"
      ;;
    *)
      release_analysis_die "Unknown option: $1"
      ;;
  esac
  shift
done

[[ -f "${PROMPT_FILE}" ]] || release_analysis_die "Missing prompt file: ${PROMPT_FILE}"
[[ -x "${METADATA_SCRIPT}" || -f "${METADATA_SCRIPT}" ]] \
  || release_analysis_die "Missing metadata helper: ${METADATA_SCRIPT}"
release_analysis_validate_reasoning_effort "${reasoning_effort}"

[[ -n "${candidate_tag}" ]] || release_analysis_die "--candidate-tag is required"
[[ -n "${from_stable_tag}" ]] || release_analysis_die "--from-stable-tag is required"

assert_release_tag() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || release_analysis_die "${label} must be an exact vX.Y.Z release tag, got '${value}'"
}

assert_release_tag "--candidate-tag" "${candidate_tag}"
assert_release_tag "--from-stable-tag" "${from_stable_tag}"

cd "${REPO_ROOT}"

git rev-parse -q --verify "refs/tags/${candidate_tag}^{commit}" >/dev/null 2>&1 \
  || release_analysis_die "Candidate tag does not exist: ${candidate_tag}"
git rev-parse -q --verify "refs/tags/${from_stable_tag}^{commit}" >/dev/null 2>&1 \
  || release_analysis_die "Stable baseline tag does not exist: ${from_stable_tag}"

[[ "$(release_analysis_compare_versions "${from_stable_tag}" "${candidate_tag}")" == "-1" ]] \
  || release_analysis_die "Stable baseline ${from_stable_tag} must be strictly older than candidate ${candidate_tag}"

git merge-base --is-ancestor "${from_stable_tag}^{commit}" "${candidate_tag}^{commit}" \
  || release_analysis_die "Stable baseline ${from_stable_tag} is not an ancestor of candidate ${candidate_tag}"

range_tags=()
while IFS= read -r tag; do
  [[ -n "${tag}" ]] || continue
  [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
  [[ "$(release_analysis_compare_versions "${from_stable_tag}" "${tag}")" == "-1" ]] || continue
  [[ "$(release_analysis_compare_versions "${tag}" "${candidate_tag}")" != "1" ]] || continue
  range_tags+=("${tag}")
done < <(git tag --list 'v[0-9]*' --sort=v:refname)

[[ "${#range_tags[@]}" -gt 0 ]] \
  || release_analysis_die "No version tags found in range (${from_stable_tag}, ${candidate_tag}]"

if [[ -z "${output_path}" ]]; then
  if [[ "${context_only}" == "true" ]]; then
    output_path=".artifacts/release-notes/stable-context-${candidate_tag}.md"
  else
    output_path=".artifacts/release-notes/stable-${candidate_tag}.md"
  fi
fi

mkdir -p "$(dirname "${output_path}")"
mkdir -p "${RELEASE_ANALYSIS_LOGS_DIR}"

tmp_dir="$(mktemp -d)"
tmp_context="${tmp_dir}/context.md"
trap 'rm -rf "${tmp_dir}"' EXIT

collected_tags=()
collected_files=()
skipped_tags=()

for tag in "${range_tags[@]}"; do
  raw_notes="${tmp_dir}/raw-${tag}.md"
  stripped_notes="${tmp_dir}/stripped-${tag}.md"
  if ! git show "${candidate_tag}:release-notes/${tag}.md" >"${raw_notes}" 2>/dev/null; then
    echo "Warning: no committed release-notes/${tag}.md in ${candidate_tag}; skipping ${tag}." >&2
    skipped_tags+=("${tag}")
    continue
  fi
  if [[ ! -s "${raw_notes}" ]]; then
    echo "Warning: release-notes/${tag}.md is empty in ${candidate_tag}; skipping ${tag}." >&2
    skipped_tags+=("${tag}")
    continue
  fi
  bash "${METADATA_SCRIPT}" --strip-only --notes-file "${raw_notes}" --output "${stripped_notes}"
  if [[ ! -s "${stripped_notes}" ]]; then
    echo "Warning: release-notes/${tag}.md is empty after metadata strip; skipping ${tag}." >&2
    skipped_tags+=("${tag}")
    continue
  fi
  collected_tags+=("${tag}")
  collected_files+=("${stripped_notes}")
done

[[ "${#collected_tags[@]}" -gt 0 ]] \
  || release_analysis_die "No committed per-build release notes found for range (${from_stable_tag}, ${candidate_tag}]"

range_spec="${from_stable_tag}..${candidate_tag}"
commit_count="$(git rev-list --count "${range_spec}")"
commit_subjects="$(git log --reverse --no-merges --pretty=format:'- %h %s' "${range_spec}")"

{
  printf 'Stable release metadata:\n'
  printf -- '- Product: RalphX.app\n'
  printf -- '- Stable release being promoted: %s\n' "${candidate_tag}"
  printf -- '- Previous Stable release: %s\n' "${from_stable_tag}"
  printf -- '- Builds included: %s\n' "$(printf '%s ' "${collected_tags[@]}" | sed 's/ $//')"
  printf -- '- Compare range: %s\n' "${range_spec}"
  printf -- '- Commit count: %s\n' "${commit_count}"
  if [[ "${#skipped_tags[@]}" -gt 0 ]]; then
    printf -- '- Tags without committed notes (covered by later builds): %s\n' \
      "$(printf '%s ' "${skipped_tags[@]}" | sed 's/ $//')"
  fi

  printf '\nReader guidance:\n'
  printf -- '- The reader is upgrading from %s directly to %s and never saw the intermediate build notes.\n' \
    "${from_stable_tag}" "${candidate_tag}"
  printf -- '- Prioritize what changes for someone who downloads, installs, opens, or uses RalphX.app.\n'
  printf -- '- Keep user-facing runtime, UI, workflow, installation, and release outcomes above developer-only changes.\n'
  printf -- '- Put developer, CI, release automation, docs, config, and scaffolding work in Developer And Maintainer Changes near the bottom.\n'

  printf '\nPer-build release notes (primary source of truth, oldest first):\n'
  for index in "${!collected_tags[@]}"; do
    printf -- '\n--- Notes for %s ---\n' "${collected_tags[${index}]}"
    cat "${collected_files[${index}]}"
    printf '\n'
  done

  if [[ -n "${commit_subjects}" ]]; then
    printf '\nCommit subjects for %s (secondary evidence, gap-filling only):\n' "${range_spec}"
    printf '%s\n' "${commit_subjects}"
  fi

  printf '\nWriter instructions for this packet:\n'
  printf -- '- Produce ONE cumulative Stable release note covering the whole %s span.\n' "${range_spec}"
  printf -- '- Merge and dedupe overlapping bullets across the per-build notes; describe the end state, not the incremental history.\n'
  printf -- '- Group by product area. Do not emit per-version subheadings or a top-level heading line.\n'
  printf -- '- Preserve the exact Markdown commit links from the source bullets you carry forward.\n'
  printf -- '- Use the commit subjects only to fill gaps where the per-build notes are sparse or missing.\n'
  printf -- '- Never introduce a change that is not present in the per-build notes or commit subjects.\n'
} > "${tmp_context}"

if [[ "${context_only}" == "true" ]]; then
  cp "${tmp_context}" "${output_path}"
  echo "Wrote stable release-notes context to ${output_path}"
  echo "Combined ${#collected_tags[@]} per-build notes from range ${range_spec}"
  exit 0
fi

command -v codex >/dev/null 2>&1 || release_analysis_die "codex CLI not found in PATH"

codex_exec_common_args=(
  --model "${model}"
  -c "model_instructions_file=\"${PROMPT_FILE}\""
  -c "model_reasoning_effort=\"${reasoning_effort}\""
  -c 'project_doc_fallback_filenames=[]'
  -c "developer_instructions=\"${RELEASE_ANALYSIS_DEVELOPER_INSTRUCTIONS}\""
  --sandbox read-only
  --ephemeral
)

output_stem="$(basename "${output_path}" .md)"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
write_log="${RELEASE_ANALYSIS_LOGS_DIR}/${output_stem}-${timestamp}-generate.log"

echo "Running stable release-notes combiner over ${#collected_tags[@]} per-build notes..."
release_analysis_run_codex_with_log "${write_log}" "${model}" "${reasoning_effort}" \
  codex exec \
  "${codex_exec_common_args[@]}" \
  --output-last-message "${output_path}" \
  - < "${tmp_context}"

[[ -s "${output_path}" ]] || release_analysis_die "Stable release notes writer produced no output at ${output_path}"

echo "Wrote combined stable release notes to ${output_path}"
echo "Generation log: ${write_log}"
