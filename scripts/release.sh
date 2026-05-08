#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMMON_FILE="${SCRIPT_DIR}/release-analysis-common.sh"
PROPOSE_SCRIPT="${SCRIPT_DIR}/propose-release.sh"
BUMP_SCRIPT="${SCRIPT_DIR}/bump-version.sh"
NOTES_SCRIPT="${SCRIPT_DIR}/generate-release-notes.sh"

usage() {
  cat <<'EOF'
Run the guided RalphX local release-prep flow.

Usage:
  ./scripts/release.sh [--current-version <version>] [--from <ref>] [--to <ref>] [--model <model>] [--reasoning-effort <low|medium|high|xhigh>] [--proposal-output <file>] [--notes-output <file>] [--allow-major]

Options:
  --current-version <version>
                        Current released version when it cannot be inferred from --from
  --from <ref>          Explicit start ref/tag/commit for the compare range
  --to <ref>            End ref/tag/commit for the compare range (default: HEAD)
  --model <model>       Codex model to use for both proposal and release notes
  --reasoning-effort <level>
                        Codex reasoning effort to use for both proposal and release notes
  --proposal-output <file>
                        Proposal markdown path (default: .artifacts/release-notes/proposal-from-v<current-version>.md)
  --notes-output <file> Release notes markdown path (default: release-notes/v<proposed-version>.md)
  --allow-major         Allow accepting a proposed major version after explicit manual approval
  -h, --help            Show this help

Flow:
  1. Generate the version proposal
  2. Pause for proposal review and acceptance
  3. Persist the accepted version to .artifacts/release-notes/.version
  4. Bump app versions
  5. Generate release notes
  6. Pause for release-notes review, then print the next manual release steps
EOF
}

source "${COMMON_FILE}"

current_version=""
from_ref=""
to_ref="HEAD"
model=""
reasoning_effort=""
proposal_output=""
notes_output=""
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
    --proposal-output)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--proposal-output requires a path"
      proposal_output="$1"
      ;;
    --notes-output)
      shift
      [[ $# -gt 0 ]] || release_analysis_die "--notes-output requires a path"
      notes_output="$1"
      ;;
    --allow-major)
      allow_major="true"
      ;;
    *)
      release_analysis_die "Unknown option: $1"
      ;;
  esac
  shift
done

[[ -x "${PROPOSE_SCRIPT}" ]] || release_analysis_die "Missing executable script: ${PROPOSE_SCRIPT}"
[[ -x "${BUMP_SCRIPT}" ]] || release_analysis_die "Missing executable script: ${BUMP_SCRIPT}"
[[ -x "${NOTES_SCRIPT}" ]] || release_analysis_die "Missing executable script: ${NOTES_SCRIPT}"
[[ -t 0 && -t 1 ]] || release_analysis_die "./scripts/release.sh is interactive. Run it in a terminal with stdin/stdout attached to a TTY."

cd "${REPO_ROOT}"

release_analysis_resolve_range "proposal-anchor" "${from_ref}" "${to_ref}"

if [[ -n "${current_version}" ]]; then
  current_version="$(release_analysis_normalize_version "${current_version}")"
else
  current_version="$(release_analysis_infer_current_version_from_ref "${RELEASE_ANALYSIS_FROM_REF}" || true)"
  [[ -n "${current_version}" ]] || release_analysis_die "Unable to infer the current released version from --from '${RELEASE_ANALYSIS_FROM_REF}'. Pass --current-version <version>."
fi

if [[ -z "${proposal_output}" ]]; then
  proposal_output="${RELEASE_ANALYSIS_STATE_DIR}/proposal-from-v${current_version}.md"
fi

proposal_args=(
  --no-prompt
  --output "${proposal_output}"
)

if [[ -n "${current_version}" ]]; then
  proposal_args+=(--current-version "${current_version}")
fi
if [[ -n "${from_ref}" ]]; then
  proposal_args+=(--from "${from_ref}")
fi
if [[ -n "${to_ref}" ]]; then
  proposal_args+=(--to "${to_ref}")
fi
if [[ -n "${model}" ]]; then
  proposal_args+=(--model "${model}")
fi
if [[ -n "${reasoning_effort}" ]]; then
  proposal_args+=(--reasoning-effort "${reasoning_effort}")
fi
if [[ "${allow_major}" == "true" ]]; then
  proposal_args+=(--allow-major)
fi

echo "Step 1/3: Generating release proposal..."
"${PROPOSE_SCRIPT}" "${proposal_args[@]}"

proposed_version="$(release_analysis_extract_proposed_version_from_file "${proposal_output}")" || release_analysis_die "Could not extract the proposed version from ${proposal_output}"

echo
echo "Review the version proposal:"
echo "  - ${proposal_output}"
printf "Accept proposed version %s and continue with bump + release-notes generation? [y/N] " "${proposed_version}"
read -r proposal_reply
case "${proposal_reply}" in
  y|Y|yes|YES)
    release_analysis_write_selected_version "${proposed_version}"
    echo "Stored accepted version in ${RELEASE_ANALYSIS_VERSION_FILE}"
    ;;
  *)
    echo "Stopped before version bump. ${RELEASE_ANALYSIS_VERSION_FILE} was not updated."
    exit 0
    ;;
esac

echo
echo "Step 2/3: Bumping repo versions..."
"${BUMP_SCRIPT}"

if [[ -z "${notes_output}" ]]; then
  notes_output="release-notes/v${proposed_version}.md"
fi

notes_args=(
  --output "${notes_output}"
)
if [[ -n "${from_ref}" ]]; then
  notes_args+=(--from "${from_ref}")
fi
if [[ -n "${to_ref}" ]]; then
  notes_args+=(--to "${to_ref}")
fi
if [[ -n "${model}" ]]; then
  notes_args+=(--model "${model}")
fi
if [[ -n "${reasoning_effort}" ]]; then
  notes_args+=(--reasoning-effort "${reasoning_effort}")
fi

echo
echo "Step 3/3: Generating release notes..."
"${NOTES_SCRIPT}" "${notes_args[@]}"

echo
echo "Review the generated files:"
echo "  - proposal: ${proposal_output}"
echo "  - accepted version: ${RELEASE_ANALYSIS_VERSION_FILE}"
echo "  - release notes: ${notes_output}"
printf "Continue after you review and edit those files? [y/N] "
read -r review_reply
case "${review_reply}" in
  y|Y|yes|YES)
    ;;
  *)
    echo "Stopped after generating review artifacts."
    exit 0
    ;;
esac

echo
echo "Next manual steps:"
echo "  git add frontend/package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json"
echo "  git commit -m \"chore: bump version to ${proposed_version}\""
echo "  git add ${notes_output}"
echo "  git commit -m \"docs: add release notes for v${proposed_version}\""
echo "  git tag v${proposed_version}"
echo "  git push origin main --tags"
