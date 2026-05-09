#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROMPT_FILE="${SCRIPT_DIR}/prompts/release-proposal-codex-prompt.md"
COMMON_FILE="${SCRIPT_DIR}/release-analysis-common.sh"
DEFAULT_MODEL="${RELEASE_PROPOSAL_MODEL:-${RELEASE_NOTES_MODEL:-gpt-5.5}}"
DEFAULT_REASONING_EFFORT="${RELEASE_PROPOSAL_REASONING_EFFORT:-${RELEASE_NOTES_REASONING_EFFORT:-xhigh}}"

usage() {
  cat <<'EOF'
Recommend the next RalphX.app release version from committed changes.

Usage:
  ./scripts/propose-release.sh [--current-version <version>] [--from <ref>] [--to <ref>] [--model <model>] [--reasoning-effort <low|medium|high|xhigh>] [--output <file>] [--accept] [--no-prompt] [--allow-major] [--context-only]

Options:
  --current-version <version>
                        Current released version when it cannot be inferred from --from
  --from <ref>          Explicit start ref/tag/commit for the compare range (default: previous tag)
  --to <ref>            End ref/tag/commit for the compare range (default: HEAD)
  --model <model>       Codex model to use (default: RELEASE_PROPOSAL_MODEL, RELEASE_NOTES_MODEL, or gpt-5.5)
  --reasoning-effort <level>
                        Codex reasoning effort to use (default: RELEASE_PROPOSAL_REASONING_EFFORT, RELEASE_NOTES_REASONING_EFFORT, or xhigh)
  --output <file>       Output markdown path (default: .artifacts/release-notes/proposal-from-v<current-version>.md)
  --accept              Persist the proposed version to .artifacts/release-notes/.version without prompting
  --no-prompt           Do not prompt to persist the proposed version
  --allow-major         Allow a proposed major version after explicit manual approval
  --context-only        Write the assembled proposal context instead of invoking Codex
  -h, --help            Show this help

Notes:
  - This should run before bumping the repo version.
  - The proposal only sees committed history in the selected git range.
  - If --from is not a release tag, pass --current-version explicitly.
  - Accepted proposals are stored in .artifacts/release-notes/.version for later scripts to reuse.
EOF
}

source "${COMMON_FILE}"

current_version=""
from_ref=""
to_ref="HEAD"
model="${DEFAULT_MODEL}"
reasoning_effort="${DEFAULT_REASONING_EFFORT}"
output_path=""
context_only="false"
accept_mode="prompt"
allow_major="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --current-version)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--current-version requires a semantic version"
      current_version="$1"
      ;;
    --from)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--from requires a ref"
      from_ref="$1"
      ;;
    --to)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--to requires a ref"
      to_ref="$1"
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
    --accept)
      accept_mode="yes"
      ;;
    --no-prompt)
      accept_mode="no"
      ;;
    --allow-major)
      allow_major="true"
      ;;
    --context-only)
      context_only="true"
      ;;
    -*)
      release_analysis_die "Unknown option: $1"
      ;;
    *)
      release_analysis_die "Unexpected argument: $1"
      ;;
  esac
  shift
done

[[ -f "${PROMPT_FILE}" ]] || release_analysis_die "Missing prompt file: ${PROMPT_FILE}"
release_analysis_validate_reasoning_effort "${reasoning_effort}"

cd "${REPO_ROOT}"

release_analysis_resolve_range "proposal-anchor" "${from_ref}" "${to_ref}"
release_analysis_collect_evidence

if [[ -n "${current_version}" ]]; then
  current_version="$(release_analysis_normalize_version "${current_version}")"
else
  current_version="$(release_analysis_infer_current_version_from_ref "${RELEASE_ANALYSIS_FROM_REF}" || true)"
  [[ -n "${current_version}" ]] || release_analysis_die "Unable to infer the current released version from --from '${RELEASE_ANALYSIS_FROM_REF}'. Pass --current-version <version>."
fi

release_analysis_compute_candidate_versions "${current_version}"

if [[ -z "${output_path}" ]]; then
  if [[ "${context_only}" == "true" ]]; then
    output_path=".artifacts/release-notes/proposal-context-from-v${current_version}.md"
  else
    output_path=".artifacts/release-notes/proposal-from-v${current_version}.md"
  fi
fi

mkdir -p "$(dirname "${output_path}")"
mkdir -p "${RELEASE_ANALYSIS_LOGS_DIR}"

reader_guidance=$'- Recommend the smallest justified bump under the provided policy.\n- Favor shipped workflow, runtime, install, and UI outcomes over repo-internal maintenance detail.\n- Treat raw commit count, diff size, and dependency churn as supporting context, not as the primary bump signal.\n- Keep the proposal decision-oriented so a human can accept or override it quickly.'

tmp_context="$(mktemp)"
tmp_final_input="$(mktemp)"
trap 'rm -f "${tmp_context}" "${tmp_final_input}"' EXIT

{
  printf 'Release proposal metadata:\n'
  printf -- '- Product: RalphX.app\n'
  printf -- '- Current released version: %s\n' "${RELEASE_ANALYSIS_CURRENT_VERSION}"
  printf -- '- Candidate patch version: %s\n' "${RELEASE_ANALYSIS_NEXT_PATCH}"
  printf -- '- Candidate minor version: %s\n' "${RELEASE_ANALYSIS_NEXT_MINOR}"
  printf -- '- Candidate major version (manual approval only): %s\n' "${RELEASE_ANALYSIS_NEXT_MAJOR}"
  printf '\nRelease evidence:\n'
  release_analysis_write_evidence_sections "${reader_guidance}"
  printf '\nVersioning policy:\n'
  printf -- '- RalphX.app uses SemVer-style numbering: MAJOR.MINOR.PATCH.\n'
  printf -- '- RalphX.app is only now starting formal public release management after an internal-only phase, and it is still in a high-volatility 0.x phase.\n'
  printf -- '- 0.x.y minor and patch numbers are unbounded integers; versions such as 0.42.0, 0.100.0, and 0.100.42 are valid and expected before 1.0.\n'
  printf -- '- Release decisions follow the shipped surface, not raw code churn.\n'
  printf -- '- Raw commit count, file count, diff size, dependency bump volume, CI churn, and release-workflow churn do not justify a larger bump by themselves.\n'
  printf -- '- While RalphX.app is on 0.x, patch is for fixes, polish, dependency churn, release/build/CI work, and internal changes that do not materially expand the shipped product surface.\n'
  printf -- '- While RalphX.app is on 0.x, minor is the normal feature release for net-new user-visible capabilities or meaningful workflow expansions, even if the product is still evolving quickly.\n'
  printf -- '- While RalphX.app is on 0.x, major is reserved for an explicit manually approved 1.0-level milestone or a deliberate compatibility reset. High volatility, large minor numbers, or release pressure are not reasons to recommend 1.0.0.\n'
  printf -- '- An unattended major recommendation is treated as a stop signal: downstream automation will fail unless a release owner manually provides the version.\n'
  printf -- '- Choose exactly one of the provided candidate versions; do not invent a different version number.\n'
  printf '\nWriter instructions for this packet:\n'
  printf -- '- Recommend the smallest bump fully justified by the release evidence.\n'
  printf -- '- Treat the current 0.x policy as binding, especially the high bar for recommending 1.0.0.\n'
  printf -- '- Prefer continued 0.x.y progression for normal feature releases, including multi-digit minor and patch numbers.\n'
  printf -- '- Use raw commit bodies as the primary source of truth and cite short SHAs inline where the supporting commit is known.\n'
  printf -- '- Keep the final proposal specific enough that a human can accept it or override it quickly.\n'
} > "${tmp_context}"

if [[ "${context_only}" == "true" ]]; then
  cp "${tmp_context}" "${output_path}"
  echo "Wrote release proposal context to ${output_path}"
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

cat "${tmp_context}" > "${tmp_final_input}"

output_stem="$(basename "${output_path}" .md)"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
write_log="${RELEASE_ANALYSIS_LOGS_DIR}/${output_stem}-${timestamp}-generate.log"

echo "Running release proposal writer..."
release_analysis_run_codex_with_log "${write_log}" "${model}" "${reasoning_effort}" \
  codex exec \
  "${codex_exec_common_args[@]}" \
  --output-last-message "${output_path}" \
  - < "${tmp_final_input}"

proposed_version="$(release_analysis_extract_proposed_version_from_file "${output_path}")" || release_analysis_die "Could not extract the proposed version from ${output_path}. Keep the proposal file, inspect the prompt contract, and retry."
release_analysis_assert_version_greater "${RELEASE_ANALYSIS_CURRENT_VERSION}" "${proposed_version}"
if [[ "${allow_major}" == "true" ]]; then
  release_analysis_assert_major_bump_allowed "${RELEASE_ANALYSIS_CURRENT_VERSION}" "${proposed_version}" "manual"
else
  release_analysis_assert_major_bump_allowed "${RELEASE_ANALYSIS_CURRENT_VERSION}" "${proposed_version}" "automatic"
fi

echo "Wrote release proposal to ${output_path}"
echo "Generation log: ${write_log}"
echo "Proposed version: ${proposed_version}"

if [[ "${accept_mode}" == "yes" ]]; then
  release_analysis_write_selected_version "${proposed_version}"
  echo "Stored accepted version in ${RELEASE_ANALYSIS_VERSION_FILE}"
elif [[ "${accept_mode}" == "prompt" && -t 0 && -t 1 ]]; then
  printf "Accept proposed version %s and write it to %s? [y/N] " "${proposed_version}" "${RELEASE_ANALYSIS_VERSION_FILE}"
  read -r accept_reply
  case "${accept_reply}" in
    y|Y|yes|YES)
      release_analysis_write_selected_version "${proposed_version}"
      echo "Stored accepted version in ${RELEASE_ANALYSIS_VERSION_FILE}"
      ;;
    *)
      echo "Left ${RELEASE_ANALYSIS_VERSION_FILE} unchanged"
      ;;
  esac
elif [[ "${accept_mode}" == "prompt" ]]; then
  echo "Left ${RELEASE_ANALYSIS_VERSION_FILE} unchanged"
  echo "Re-run with --accept or answer the interactive prompt to persist the proposal for later scripts"
fi
