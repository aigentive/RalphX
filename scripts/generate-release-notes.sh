#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROMPT_FILE="${SCRIPT_DIR}/prompts/release-notes-codex-prompt.md"
COMMON_FILE="${SCRIPT_DIR}/release-analysis-common.sh"
DEFAULT_MODEL="${RELEASE_NOTES_MODEL:-gpt-5.5}"
DEFAULT_REASONING_EFFORT="${RELEASE_NOTES_REASONING_EFFORT:-xhigh}"

usage() {
  cat <<'EOF'
Generate draft RalphX.app release notes with Codex CLI.

Usage:
  ./scripts/generate-release-notes.sh [<version>] [--from <ref>] [--to <ref>] [--model <model>] [--reasoning-effort <low|medium|high|xhigh>] [--output <file>] [--context-only]

Arguments:
  <version>             Release version without or with leading v (for example 0.2.0 or v0.2.0). If omitted, uses .artifacts/release-notes/.version.

Options:
  --from <ref>          Explicit start ref/tag/commit for the compare range
  --to <ref>            End ref/tag/commit for the compare range (default: HEAD)
  --model <model>       Codex model to use (default: RELEASE_NOTES_MODEL or gpt-5.5)
  --reasoning-effort <level>
                        Codex reasoning effort to use (default: RELEASE_NOTES_REASONING_EFFORT or xhigh)
  --output <file>       Output markdown path (default: release-notes/v<version>.md)
  --context-only        Write the assembled release context instead of invoking Codex
  -h, --help            Show this help

Notes:
  - Run ./scripts/propose-release.sh and accept the version before drafting notes if you want to omit <version>.
  - The generator only sees committed history in the selected git range.
  - Commit any release-affecting changes you want reflected before running it.
EOF
}

source "${COMMON_FILE}"

version=""
from_ref=""
to_ref="HEAD"
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
    --context-only)
      context_only="true"
      ;;
    -*)
      release_analysis_die "Unknown option: $1"
      ;;
    *)
      if [[ -n "${version}" ]]; then
        release_analysis_die "Version already set to '${version}', unexpected argument '$1'"
      fi
      version="$1"
      ;;
  esac
  shift
done

[[ -f "${PROMPT_FILE}" ]] || release_analysis_die "Missing prompt file: ${PROMPT_FILE}"
release_analysis_validate_reasoning_effort "${reasoning_effort}"

cd "${REPO_ROOT}"

if [[ -z "${version}" ]]; then
  version="$(release_analysis_read_selected_version_or_die)"
  echo "Using stored release version ${version} from ${RELEASE_ANALYSIS_VERSION_FILE}"
fi

raw_version="$(release_analysis_normalize_version "${version}")"
tag="v${raw_version}"

release_analysis_resolve_range "${tag}" "${from_ref}" "${to_ref}"
release_analysis_collect_evidence

if [[ -z "${output_path}" ]]; then
  if [[ "${context_only}" == "true" ]]; then
    output_path=".artifacts/release-notes/context-${tag}.md"
  else
    output_path="release-notes/${tag}.md"
  fi
fi

mkdir -p "$(dirname "${output_path}")"
mkdir -p "${RELEASE_ANALYSIS_LOGS_DIR}"

reader_guidance=$'- Target multiple audiences at once: public readers, active users, contributors, and maintainers.\n- Prioritize what changes for someone who downloads, installs, opens, or uses RalphX.app.\n- Keep user-facing runtime, UI, workflow, installation, and release outcomes above developer-only changes.\n- Put developer, CI, release automation, docs, config, and scaffolding work in a separate Developer And Maintainer Changes section near the bottom when it is worth mentioning.\n- Keep the tone precise and engineering-literate without drifting into repo-internal maintenance detail.'

tmp_context="$(mktemp)"
tmp_final_input="$(mktemp)"
trap 'rm -f "${tmp_context}" "${tmp_final_input}"' EXIT

{
  printf 'Release metadata:\n'
  printf -- '- Product: RalphX.app\n'
  printf -- '- Version: %s\n' "${raw_version}"
  printf -- '- Tag: %s\n' "${tag}"
  printf '\nRelease evidence:\n'
  release_analysis_write_evidence_sections "${reader_guidance}"
  printf '\nWriter instructions for this packet:\n'
  printf -- '- Use the raw commit bodies as the primary source of truth.\n'
  printf -- '- Group related bullets into coherent product areas instead of echoing commit subjects line by line.\n'
  printf -- '- Use commit subjects and diff stat only to fill gaps when the raw bodies are sparse.\n'
  printf -- '- Do not assume every `feat:` bullet means a net-new surface; many are expansions of existing behavior.\n'
  printf -- '- Keep internal-only work out of User-Facing Changes and Fixes And Polish unless the shipped impact is explicit in the commit bodies.\n'
  printf -- '- Clearly separate developer-facing work under Developer And Maintainer Changes after the user-facing sections.\n'
} > "${tmp_context}"

if [[ "${context_only}" == "true" ]]; then
  cp "${tmp_context}" "${output_path}"
  echo "Wrote release-notes context to ${output_path}"
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

echo "Running release-notes writer..."
release_analysis_run_codex_with_log "${write_log}" "${model}" "${reasoning_effort}" \
  codex exec \
  "${codex_exec_common_args[@]}" \
  --output-last-message "${output_path}" \
  - < "${tmp_final_input}"

echo "Wrote draft release notes to ${output_path}"
echo "Generation log: ${write_log}"
