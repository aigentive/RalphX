#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_TMP="$(mktemp -d)"
STATE_DIR="${TEST_TMP}/state"
FAKE_BIN="${TEST_TMP}/bin"
REAL_SHASUM="$(command -v shasum)"

cleanup() {
  rm -rf "${TEST_TMP}"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_equals() {
  local expected="$1"
  local actual="$2"
  [[ "${actual}" == "${expected}" ]] || fail "expected '${expected}', got '${actual}'"
}

assert_fails() {
  local label="$1"
  shift
  if "$@" >"${TEST_TMP}/${label}.log" 2>&1; then
    fail "${label} unexpectedly succeeded"
  fi
}

assert_mutation_precedes() {
  local first="$1"
  local second="$2"
  local first_line second_line
  first_line="$(grep -nFx -- "${first}" "${STATE_DIR}/mutations.log" | head -n1 | cut -d: -f1)"
  second_line="$(grep -nFx -- "${second}" "${STATE_DIR}/mutations.log" | head -n1 | cut -d: -f1)"
  [[ -n "${first_line}" && -n "${second_line}" && "${first_line}" -lt "${second_line}" ]] \
    || fail "expected mutation ${first} before ${second}"
}

assert_pointer_assets_absent() {
  local asset_dir="${STATE_DIR}/releases/updater-stable/assets"
  [[ ! -d "${asset_dir}" || -z "$(find "${asset_dir}" -maxdepth 1 -type f -print -quit)" ]] \
    || fail "updater-stable pointer assets changed unexpectedly"
}

mkdir -p "${STATE_DIR}/releases" "${FAKE_BIN}"

cat >"${FAKE_BIN}/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

state="${FAKE_GH_STATE:?FAKE_GH_STATE is required}"

release_path() {
  printf '%s/releases/%s/release.json\n' "${state}" "$1"
}

release_assets_json() {
  local tag="$1"
  local asset_dir="${state}/releases/${tag}/assets"
  if [[ ! -d "${asset_dir}" ]]; then
    printf '[]\n'
    return
  fi
  find "${asset_dir}" -maxdepth 1 -type f -exec basename {} \; \
    | LC_ALL=C sort \
    | jq -R . \
    | jq -s 'map({name: .})'
}

release_json() {
  local tag="$1"
  local path
  path="$(release_path "${tag}")"
  [[ -f "${path}" ]] || exit 1
  jq --argjson assets "$(release_assets_json "${tag}")" '. + {assets: $assets}' "${path}"
}

set_release_fields() {
  local tag="$1"
  shift
  local path tmp
  path="$(release_path "${tag}")"
  tmp="${path}.tmp"
  jq "$@" "${path}" >"${tmp}"
  mv "${tmp}" "${path}"
}

log_mutation() {
  printf '%s\n' "$1" >>"${state}/mutations.log"
}

command_name="${1:-}"
shift || true
case "${command_name}" in
  release)
    subcommand="${1:-}"
    shift || true
    case "${subcommand}" in
      view)
        [[ "$*" != *isLatest* ]] || {
          echo "fake gh: isLatest is unsupported for gh release view" >&2
          exit 42
        }
        release_json "$1"
        ;;
      list)
        echo "fake gh: release list must not be used for prior Stable derivation" >&2
        exit 42
        ;;
      download)
        tag="$1"
        shift
        pattern=""
        destination=""
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --repo)
              shift 2
              ;;
            --pattern)
              pattern="$2"
              shift 2
              ;;
            --dir)
              destination="$2"
              shift 2
              ;;
            *)
              shift
              ;;
          esac
        done
        [[ -n "${pattern}" && -n "${destination}" && -f "${state}/releases/${tag}/assets/${pattern}" ]] || {
          echo "fake gh download missing ${tag}/${pattern}" >&2
          exit 1
        }
        mkdir -p "${destination}"
        cp "${state}/releases/${tag}/assets/${pattern}" "${destination}/${pattern}"
        ;;
      create)
        tag="$1"
        [[ " $* " == *" --prerelease "* && " $* " == *" --latest=false "* ]] || {
          echo "fake gh: pointer infrastructure must be a published non-latest prerelease" >&2
          exit 42
        }
        [[ "${FAKE_GH_FAIL_CREATE_TAG:-}" != "${tag}" ]] || exit 1
        mkdir -p "${state}/releases/${tag}/assets"
        immutable=false
        [[ "${FAKE_GH_CREATE_IMMUTABLE_TAG:-}" != "${tag}" ]] || immutable=true
        jq -n --arg tag "${tag}" --argjson immutable "${immutable}" '{tagName: $tag, isDraft: false, isPrerelease: true, isLatest: false, immutable: $immutable}' >"$(release_path "${tag}")"
        log_mutation "create:${tag}"
        ;;
      edit)
        tag="$1"
        shift
        path="$(release_path "${tag}")"
        [[ -f "${path}" ]] || exit 1
        log_mutation "edit:${tag}"
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --repo|--title)
              shift 2
              ;;
            --notes-file)
              cp "$2" "${state}/releases/${tag}/body.md"
              log_mutation "body:${tag}"
              shift 2
              ;;
            --prerelease)
              set_release_fields "${tag}" '.isPrerelease = true'
              shift
              ;;
            --prerelease=false)
              set_release_fields "${tag}" '.isPrerelease = false'
              shift
              ;;
            --latest=true)
              for other_path in "${state}"/releases/*/release.json; do
                tmp="${other_path}.tmp"
                jq '.isLatest = false' "${other_path}" >"${tmp}"
                mv "${tmp}" "${other_path}"
              done
              set_release_fields "${tag}" '.isLatest = true'
              shift
              ;;
            --latest=false)
              set_release_fields "${tag}" '.isLatest = false'
              shift
              ;;
            *)
              shift
              ;;
          esac
        done
        ;;
      upload)
        tag="$1"
        shift
        destination="${state}/releases/${tag}/assets"
        [[ -d "${destination}" ]] || exit 1
        log_mutation "upload:${tag}"
        while [[ $# -gt 0 ]]; do
          case "$1" in
            --repo)
              shift 2
              ;;
            --clobber)
              shift
              ;;
            *)
              cp "$1" "${destination}/$(basename "$1")"
              # Fault injection: simulate a release asset that does not match what was uploaded.
              if [[ "${FAKE_GH_CORRUPT_UPLOAD_TAG:-}" == "${tag}" \
                && "$(basename "$1")" == "latest.json" ]]; then
                printf 'corrupted\n' >>"${destination}/latest.json"
              fi
              shift
              ;;
          esac
        done
        ;;
      *)
        exit 1
        ;;
    esac
    ;;
  api)
    endpoint=""
    slurp=false
    paginated=false
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --paginate)
          paginated=true
          shift
          ;;
        --slurp)
          slurp=true
          shift
          ;;
        --jq)
          shift 2
          ;;
        *)
          endpoint="$1"
          shift
          ;;
      esac
    done
    case "${endpoint}" in
      */releases\?per_page=100)
        [[ "${paginated}" == "true" && "${slurp}" == "true" ]] || {
          echo "fake gh: prior Stable derivation must use paginated slurped REST data" >&2
          exit 42
        }
        release_files=("${state}"/releases/*/release.json)
        if [[ ! -e "${release_files[0]}" ]]; then
          releases='[]'
        else
          releases='[]'
          for path in "${release_files[@]}"; do
            rest_release="$(jq '{tag_name: .tagName, draft: .isDraft, prerelease: .isPrerelease}' "${path}")"
            releases="$(jq --argjson release "${rest_release}" '. + [$release]' <<<"${releases}")"
          done
        fi
        if [[ "${slurp}" == "true" ]]; then
          jq -cn --argjson releases "${releases}" '[$releases]'
        else
          printf '%s\n' "${releases}"
        fi
        ;;
      */releases/tags/*)
        tag="${endpoint##*/}"
        path="$(release_path "${tag}")"
        [[ -f "${path}" ]] || exit 1
        cat "${path}"
        ;;
      */releases/latest)
        release_files=("${state}"/releases/*/release.json)
        [[ -e "${release_files[0]}" ]] || exit 1
        latest=""
        for path in "${release_files[@]}"; do
          if [[ "$(jq -r '.isDraft == false and .isPrerelease == false and .isLatest == true' "${path}")" == "true" ]]; then
            latest="${path}"
          fi
        done
        [[ -n "${latest}" ]] || exit 1
        jq '{tag_name: .tagName, draft: .isDraft, prerelease: .isPrerelease}' "${latest}"
        ;;
      *)
        exit 1
        ;;
    esac
    ;;
  *)
    exit 1
    ;;
esac
EOF

cat >"${FAKE_BIN}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

state="${FAKE_GH_STATE:?FAKE_GH_STATE is required}"
mode="${FAKE_CURL_MODE:-current}"
output=""
url=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    --connect-timeout|--max-time)
      shift 2
      ;;
    --fail|--location|--silent|--show-error)
      shift
      ;;
    *)
      url="$1"
      shift
      ;;
  esac
done
[[ "${url}" =~ /releases/download/(updater-stable|updater-nightly)/(latest-aarch64\.json|latest-x86_64\.json)$ ]] || exit 1
pointer_tag="${BASH_REMATCH[1]}"
asset="${BASH_REMATCH[2]}"
count_file="${state}/curl-${pointer_tag}-${asset}.count"
count=0
[[ -f "${count_file}" ]] && count="$(cat "${count_file}")"
count=$((count + 1))
printf '%s\n' "${count}" >"${count_file}"

case "${mode}" in
  current)
    cp "${state}/releases/${pointer_tag}/assets/${asset}" "${output}"
    ;;
  stale-once)
    if [[ "${count}" -eq 1 ]]; then
      cp "${state}/stale/${asset}" "${output}"
    else
      cp "${state}/releases/${pointer_tag}/assets/${asset}" "${output}"
    fi
    ;;
  always-stale)
    cp "${state}/stale/${asset}" "${output}"
    ;;
  *)
    exit 1
    ;;
esac
EOF

cat >"${FAKE_BIN}/shasum" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

[[ "${FAKE_SHASUM_FAIL:-}" != "true" ]] || exit 1
exec "${REAL_SHASUM:?REAL_SHASUM is required}" "$@"
EOF
chmod +x "${FAKE_BIN}/gh" "${FAKE_BIN}/curl" "${FAKE_BIN}/shasum"

reset_state() {
  rm -rf "${STATE_DIR}/releases" "${STATE_DIR}/stale" "${STATE_DIR}"/*.count "${STATE_DIR}/mutations.log"
  mkdir -p "${STATE_DIR}/releases"
  : >"${STATE_DIR}/mutations.log"
  TEST_CURL_MODE="current"
  TEST_ATTEMPTS="1"
  TEST_DELAY_SECONDS="0"
  TEST_GH_FAIL_CREATE_TAG=""
  TEST_GH_CREATE_IMMUTABLE_TAG=""
  TEST_SHASUM_FAIL="false"
  TEST_GH_CORRUPT_UPLOAD_TAG=""
}

make_release() {
  local tag="$1"
  local prerelease="$2"
  local latest="$3"
  mkdir -p "${STATE_DIR}/releases/${tag}/assets"
  jq -n \
    --arg tag "${tag}" \
    --argjson prerelease "${prerelease}" \
    --argjson latest "${latest}" \
    '{tagName: $tag, isDraft: false, isPrerelease: $prerelease, isLatest: $latest, immutable: false}' \
    >"${STATE_DIR}/releases/${tag}/release.json"
}

set_release_field() {
  local tag="$1"
  local filter="$2"
  local path="${STATE_DIR}/releases/${tag}/release.json"
  jq "${filter}" "${path}" >"${path}.tmp"
  mv "${path}.tmp" "${path}"
}

make_source_assets() {
  local tag="$1"
  local version="${tag#v}"
  local assets="${STATE_DIR}/releases/${tag}/assets"
  local notes="${TEST_TMP}/notes-${tag}.md"
  local manifests="${TEST_TMP}/source-manifests-${tag}"
  mkdir -p "${assets}" "${manifests}"
  printf 'notes for %s\n' "${tag}" >"${notes}"
  printf 'arm-%s' "${tag}" >"${assets}/RalphX_${version}_aarch64.app.tar.gz"
  printf 'arm-signature-%s' "${tag}" >"${assets}/RalphX_${version}_aarch64.app.tar.gz.sig"
  printf 'arm-dmg-%s' "${tag}" >"${assets}/RalphX_${version}_aarch64.dmg"
  printf 'intel-%s' "${tag}" >"${assets}/RalphX_${version}_x86_64.app.tar.gz"
  printf 'intel-signature-%s' "${tag}" >"${assets}/RalphX_${version}_x86_64.app.tar.gz.sig"
  printf 'intel-dmg-%s' "${tag}" >"${assets}/RalphX_${version}_x86_64.dmg"
  printf 'checksums-%s\n' "${tag}" >"${assets}/checksums.txt"
  bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
    --tag "${tag}" \
    --notes-file "${notes}" \
    --aarch64-signature "${assets}/RalphX_${version}_aarch64.app.tar.gz.sig" \
    --x86_64-signature "${assets}/RalphX_${version}_x86_64.app.tar.gz.sig" \
    --pub-date '2026-07-23T10:00:00Z' \
    --channel stable \
    --output-dir "${manifests}"
  cp "${manifests}/latest.json" "${assets}/latest.json"
}

render_stable_pointers() {
  local tag="$1"
  local version="${tag#v}"
  local source_assets="${STATE_DIR}/releases/${tag}/assets"
  local rendered="${TEST_TMP}/pointers-${tag}"
  local notes="${TEST_TMP}/notes-${tag}.md"
  mkdir -p "${rendered}"
  bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
    --tag "${tag}" \
    --notes-file "${notes}" \
    --aarch64-signature "${source_assets}/RalphX_${version}_aarch64.app.tar.gz.sig" \
    --x86_64-signature "${source_assets}/RalphX_${version}_x86_64.app.tar.gz.sig" \
    --pub-date '2026-07-23T10:00:00Z' \
    --channel stable \
    --output-dir "${rendered}"
}

render_nightly_pointers() {
  local tag="$1"
  local version="${tag#v}"
  local source_assets="${STATE_DIR}/releases/${tag}/assets"
  local rendered="${TEST_TMP}/nightly-pointers-${tag}"
  local notes="${TEST_TMP}/notes-${tag}.md"
  mkdir -p "${rendered}"
  bash "${ROOT_DIR}/scripts/render-updater-channel-manifests.sh" \
    --tag "${tag}" \
    --notes-file "${notes}" \
    --aarch64-signature "${source_assets}/RalphX_${version}_aarch64.app.tar.gz.sig" \
    --x86_64-signature "${source_assets}/RalphX_${version}_x86_64.app.tar.gz.sig" \
    --pub-date '2026-07-23T10:00:00Z' \
    --channel nightly \
    --output-dir "${rendered}"
}

make_stable_pointers() {
  local tag="$1"
  local pointer_assets="${STATE_DIR}/releases/updater-stable/assets"
  render_stable_pointers "${tag}"
  mkdir -p "${pointer_assets}"
  rm -f "${pointer_assets}"/*
  cp "${TEST_TMP}/pointers-${tag}/latest-aarch64.json" "${pointer_assets}/latest-aarch64.json"
  cp "${TEST_TMP}/pointers-${tag}/latest-x86_64.json" "${pointer_assets}/latest-x86_64.json"
}

make_nightly_pointers() {
  local tag="$1"
  local pointer_assets="${STATE_DIR}/releases/updater-nightly/assets"
  render_nightly_pointers "${tag}"
  mkdir -p "${pointer_assets}"
  rm -f "${pointer_assets}"/*
  cp "${TEST_TMP}/nightly-pointers-${tag}/latest-aarch64.json" "${pointer_assets}/latest-aarch64.json"
  cp "${TEST_TMP}/nightly-pointers-${tag}/latest-x86_64.json" "${pointer_assets}/latest-x86_64.json"
}

set_pointer_arch() {
  local arch="$1"
  local tag="$2"
  render_stable_pointers "${tag}"
  cp "${TEST_TMP}/pointers-${tag}/latest-${arch}.json" \
    "${STATE_DIR}/releases/updater-stable/assets/latest-${arch}.json"
}

set_nightly_pointer_arch() {
  local arch="$1"
  local tag="$2"
  render_nightly_pointers "${tag}"
  cp "${TEST_TMP}/nightly-pointers-${tag}/latest-${arch}.json" \
    "${STATE_DIR}/releases/updater-nightly/assets/latest-${arch}.json"
}

release_state() {
  local tag="$1"
  jq -r '[.isDraft, .isPrerelease, .isLatest] | @tsv' "${STATE_DIR}/releases/${tag}/release.json"
}

assert_release_state() {
  assert_equals "$2" "$(release_state "$1")"
}

assert_pointer_versions() {
  local tag="$1"
  assert_equals "${tag}" "$(jq -r '.version' "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json")"
  assert_equals "${tag}" "$(jq -r '.version' "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json")"
}

assert_nightly_pointer_versions() {
  local tag="$1"
  assert_equals "${tag}" "$(jq -r '.version' "${STATE_DIR}/releases/updater-nightly/assets/latest-aarch64.json")"
  assert_equals "${tag}" "$(jq -r '.version' "${STATE_DIR}/releases/updater-nightly/assets/latest-x86_64.json")"
}

assert_no_mutations() {
  [[ ! -s "${STATE_DIR}/mutations.log" ]] || fail "rejection mutated release state: $(tr '\n' ' ' <"${STATE_DIR}/mutations.log")"
}

run_control() {
  local operation="$1"
  local candidate="$2"
  local bad="$3"
  local restore="$4"
  FAKE_GH_STATE="${STATE_DIR}" \
    FAKE_GH_FAIL_CREATE_TAG="${TEST_GH_FAIL_CREATE_TAG}" \
    FAKE_GH_CREATE_IMMUTABLE_TAG="${TEST_GH_CREATE_IMMUTABLE_TAG}" \
    FAKE_SHASUM_FAIL="${TEST_SHASUM_FAIL}" \
    REAL_SHASUM="${REAL_SHASUM}" \
    RALPHX_TRACE_NIGHTLY_POINTERS="${RALPHX_TRACE_NIGHTLY_POINTERS:-}" \
    FAKE_CURL_MODE="${TEST_CURL_MODE}" \
    RELEASE_PUBLIC_VERIFY_ATTEMPTS="${TEST_ATTEMPTS}" \
    RELEASE_PUBLIC_VERIFY_DELAY_SECONDS="${TEST_DELAY_SECONDS}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT_DIR}/scripts/reconcile-stable-release-state.sh" \
      --operation "${operation}" \
      --candidate-tag "${candidate}" \
      --bad-tag "${bad}" \
      --restore-tag "${restore}" \
      --repo example/ralphx \
      --work-dir "${TEST_TMP}/control-${operation}" \
      --output "${TEST_TMP}/result-${operation}.env"
}

run_control_with_notes() {
  local operation="$1"
  local candidate="$2"
  local bad="$3"
  local restore="$4"
  local body_notes="$5"
  local pointer_notes="$6"
  local extra_args=()
  [[ -z "${body_notes}" ]] || extra_args+=(--promote-body-notes-file "${body_notes}")
  [[ -z "${pointer_notes}" ]] || extra_args+=(--pointer-notes-file "${pointer_notes}")
  FAKE_GH_STATE="${STATE_DIR}" \
    FAKE_GH_FAIL_CREATE_TAG="${TEST_GH_FAIL_CREATE_TAG}" \
    FAKE_GH_CREATE_IMMUTABLE_TAG="${TEST_GH_CREATE_IMMUTABLE_TAG}" \
    FAKE_GH_CORRUPT_UPLOAD_TAG="${TEST_GH_CORRUPT_UPLOAD_TAG}" \
    FAKE_SHASUM_FAIL="${TEST_SHASUM_FAIL}" \
    REAL_SHASUM="${REAL_SHASUM}" \
    FAKE_CURL_MODE="${TEST_CURL_MODE}" \
    RELEASE_PUBLIC_VERIFY_ATTEMPTS="${TEST_ATTEMPTS}" \
    RELEASE_PUBLIC_VERIFY_DELAY_SECONDS="${TEST_DELAY_SECONDS}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT_DIR}/scripts/reconcile-stable-release-state.sh" \
      --operation "${operation}" \
      --candidate-tag "${candidate}" \
      --bad-tag "${bad}" \
      --restore-tag "${restore}" \
      --repo example/ralphx \
      --work-dir "${TEST_TMP}/control-notes-${operation}-$$-${RANDOM}" \
      --output "${TEST_TMP}/result-notes-${operation}.env" \
      "${extra_args[@]}"
}

write_combined_notes() {
  local stem="$1"
  printf 'Combined %s body.\n\n## User-Facing Changes\n- Merged bullet.\n' "${stem}" \
    >"${TEST_TMP}/${stem}-body.md"
  printf 'Combined %s updater.\n' "${stem}" >"${TEST_TMP}/${stem}-updater.md"
}

assert_release_body() {
  local tag="$1"
  local expected_file="$2"
  cmp -s "${STATE_DIR}/releases/${tag}/body.md" "${expected_file}" \
    || fail "release body for ${tag} does not match ${expected_file}"
}

assert_manifest_notes() {
  local manifest="$1"
  local expected_file="$2"
  local actual="${TEST_TMP}/actual-notes.$$"
  jq -jr '.notes' "${manifest}" >"${actual}"
  cmp -s "${actual}" "${expected_file}" \
    || fail "notes in ${manifest} do not match ${expected_file}"
  rm -f "${actual}"
}

# Pointer notes derived by reading a manifest back (every halt, and every promotion without
# combined-notes overrides) pass through `jq -r '.notes'`, which appends one trailing newline.
# That is long-standing behavior on those paths, so compare notes content rather than bytes there.
assert_manifest_notes_content() {
  local manifest="$1"
  local expected_file="$2"
  local actual="${TEST_TMP}/actual-notes-content.$$"
  jq -jr '.notes' "${manifest}" | sed -e :a -e '/^\n*$/{$d;N;};/\n$/ba' >"${actual}"
  diff <(sed -e :a -e '/^\n*$/{$d;N;};/\n$/ba' "${expected_file}") "${actual}" >/dev/null \
    || fail "notes content in ${manifest} does not match ${expected_file}"
  rm -f "${actual}"
}

assert_manifest_identical_except_notes() {
  local left="$1"
  local right="$2"
  diff <(jq -S 'del(.notes)' "${left}") <(jq -S 'del(.notes)' "${right}") >/dev/null \
    || fail "${left} and ${right} differ outside .notes"
}

run_fake_gh() {
  FAKE_GH_STATE="${STATE_DIR}" PATH="${FAKE_BIN}:${PATH}" gh "$@"
}

run_public_verify() {
  local pointer_tag="$1"
  local expected_dir="$2"
  FAKE_GH_STATE="${STATE_DIR}" \
    FAKE_CURL_MODE="${TEST_CURL_MODE}" \
    RELEASE_PUBLIC_VERIFY_ATTEMPTS="${TEST_ATTEMPTS}" \
    RELEASE_PUBLIC_VERIFY_DELAY_SECONDS="${TEST_DELAY_SECONDS}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT_DIR}/scripts/verify-public-updater-pointers.sh" \
      example/ralphx "${pointer_tag}" "${expected_dir}"
}

run_nightly_control() {
  local candidate="$1"
  local upload_dir="${TEST_TMP}/nightly-upload-${candidate}"
  render_nightly_pointers "${candidate}"
  mkdir -p "${upload_dir}"
  cp "${TEST_TMP}/nightly-pointers-${candidate}/latest-aarch64.json" "${upload_dir}/latest-aarch64.json"
  cp "${TEST_TMP}/nightly-pointers-${candidate}/latest-x86_64.json" "${upload_dir}/latest-x86_64.json"
  FAKE_GH_STATE="${STATE_DIR}" \
    FAKE_CURL_MODE="${TEST_CURL_MODE}" \
    RELEASE_PUBLIC_VERIFY_ATTEMPTS="${TEST_ATTEMPTS}" \
    RELEASE_PUBLIC_VERIFY_DELAY_SECONDS="${TEST_DELAY_SECONDS}" \
    PATH="${FAKE_BIN}:${PATH}" \
    bash "${ROOT_DIR}/scripts/reconcile-nightly-updater-pointers.sh" \
      --repo example/ralphx \
      --candidate-tag "${candidate}" \
      --source-dir "${STATE_DIR}/releases/${candidate}/assets" \
      --pointer-dir "${upload_dir}" \
      --work-dir "${TEST_TMP}/nightly-control"
}

setup_promotable_pair() {
  reset_state
  make_release v1.0.0 false true
  make_source_assets v1.0.0
  make_release v1.1.0 true false
  make_source_assets v1.1.0
}

setup_haltable_state() {
  reset_state
  make_release v1.0.0 false false
  make_source_assets v1.0.0
  make_release v1.1.0 false false
  make_source_assets v1.1.0
  make_release v1.2.0 false true
  make_source_assets v1.2.0
  make_release updater-stable true false
  make_stable_pointers v1.2.0
}

setup_nightly_pair() {
  reset_state
  make_release v1.0.0 true false
  make_source_assets v1.0.0
  make_release v1.1.0 true false
  make_source_assets v1.1.0
}

pointer_test_scope="${RELEASE_POINTER_TEST_SCOPE:-all}"

# The fake protects production from unsupported isLatest fields even in comma-separated --json lists.
reset_state
make_release v1.0.0 false true
assert_fails rejects_comma_separated_is_latest \
  run_fake_gh release view v1.0.0 --repo example/ralphx --json tagName,isLatest

# First promotion pre-creates mutable pointer infrastructure before advancing GitHub authority.
if [[ "${pointer_test_scope}" == "all" || "${pointer_test_scope}" == "stable" || "${pointer_test_scope}" == "stable-core" ]]; then
  setup_promotable_pair
  run_control promote v1.1.0 "" ""
  assert_release_state v1.1.0 $'false\tfalse\ttrue'
  assert_release_state v1.0.0 $'false\tfalse\tfalse'
  assert_release_state updater-stable $'false\ttrue\tfalse'
  assert_mutation_precedes create:updater-stable edit:v1.1.0
  grep -Fq 'version "1.1.0"' "${TEST_TMP}/control-promote/staged-homebrew-cask.rb" \
    || fail "promotion did not stage the selected target Homebrew cask"
  assert_pointer_versions v1.1.0

# Later promotion preserves prior full releases as history and exact reruns converge.
make_release v1.2.0 true false
make_source_assets v1.2.0
run_control promote v1.2.0 "" ""
assert_release_state v1.2.0 $'false\tfalse\ttrue'
assert_release_state v1.1.0 $'false\tfalse\tfalse'
assert_pointer_versions v1.2.0
run_control promote v1.2.0 "" ""
assert_release_state v1.2.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.2.0

# A candidate whose GitHub authority advanced before pointer upload is repaired, even with an empty pointer release.
setup_promotable_pair
set_release_field v1.0.0 '.isLatest = false'
set_release_field v1.1.0 '.isPrerelease = false | .isLatest = true'
make_release updater-stable true false
run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.1.0

# A one-architecture pointer update is accepted only when GitHub already proves the requested promotion.
setup_promotable_pair
set_release_field v1.0.0 '.isLatest = false'
set_release_field v1.1.0 '.isPrerelease = false | .isLatest = true'
make_release updater-stable true false
make_stable_pointers v1.0.0
set_pointer_arch aarch64 v1.1.0
run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.1.0

# A candidate-full/latest state may repair a sole candidate pointer even when a prior Stable exists.
setup_promotable_pair
set_release_field v1.0.0 '.isLatest = false'
set_release_field v1.1.0 '.isPrerelease = false | .isLatest = true'
make_release updater-stable true false
make_stable_pointers v1.0.0
set_pointer_arch aarch64 v1.1.0
rm "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json"
run_control promote v1.1.0 "" ""
assert_pointer_versions v1.1.0

# The same bounded recovery rejects a one-asset pointer that still names the prior Stable release.
setup_promotable_pair
set_release_field v1.0.0 '.isLatest = false'
set_release_field v1.1.0 '.isPrerelease = false | .isLatest = true'
make_release updater-stable true false
make_stable_pointers v1.0.0
rm "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json"
assert_fails rejects_prior_only_half_pointer run_control promote v1.1.0 "" ""
assert_no_mutations

# Unrelated mixed pointers fail closed and do not mutate an already-advanced candidate.
reset_state
make_release v1.0.0 false false
make_source_assets v1.0.0
make_release v1.1.0 false false
make_source_assets v1.1.0
make_release v1.2.0 false true
make_source_assets v1.2.0
make_release updater-stable true false
make_stable_pointers v1.0.0
set_pointer_arch aarch64 v1.2.0
assert_fails rejects_unrelated_pointer_disagreement run_control promote v1.2.0 "" ""
assert_release_state v1.2.0 $'false\tfalse\ttrue'
assert_equals v1.2.0 "$(jq -r '.version' "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json")"
assert_equals v1.0.0 "$(jq -r '.version' "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json")"
assert_no_mutations

# Halt moves GitHub authority to the derived restore first, then restores both pointers; completed reruns are safe.
setup_haltable_state
make_release updater-nightly false false
run_control halt "" v1.2.0 v1.1.0
assert_release_state v1.2.0 $'false\ttrue\tfalse'
assert_release_state v1.1.0 $'false\tfalse\ttrue'
grep -Fq 'version "1.1.0"' "${TEST_TMP}/control-halt/staged-homebrew-cask.rb" \
  || fail "halt did not stage the selected restore Homebrew cask"
assert_pointer_versions v1.1.0
run_control halt "" v1.2.0 v1.1.0
assert_release_state v1.2.0 $'false\ttrue\tfalse'
assert_release_state v1.1.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.1.0

# Halt repairs GitHub-advanced old pointers and a one-architecture restore without accepting other combinations.
setup_haltable_state
set_release_field v1.2.0 '.isPrerelease = true | .isLatest = false'
set_release_field v1.1.0 '.isLatest = true'
run_control halt "" v1.2.0 v1.1.0
assert_pointer_versions v1.1.0

setup_haltable_state
set_release_field v1.2.0 '.isPrerelease = true | .isLatest = false'
set_release_field v1.1.0 '.isLatest = true'
set_pointer_arch aarch64 v1.1.0
run_control halt "" v1.2.0 v1.1.0
assert_release_state v1.2.0 $'false\ttrue\tfalse'
assert_release_state v1.1.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.1.0

setup_haltable_state
assert_fails rejects_derived_restore_mismatch run_control halt "" v1.2.0 v1.0.0
assert_release_state v1.2.0 $'false\tfalse\ttrue'
assert_pointer_versions v1.2.0
assert_no_mutations

# Draft/immutable candidates and invalid source assets fail before any GitHub mutation.
setup_promotable_pair
set_release_field v1.1.0 '.isDraft = true'
assert_fails rejects_draft_candidate run_control promote v1.1.0 "" ""
assert_no_mutations

setup_promotable_pair
set_release_field v1.1.0 '.immutable = true'
assert_fails rejects_immutable_candidate run_control promote v1.1.0 "" ""
assert_no_mutations

# First-pointer creation or validation failure cannot advance version authority or upload pointer assets.
setup_promotable_pair
TEST_GH_FAIL_CREATE_TAG=updater-stable
assert_fails rejects_first_pointer_creation_failure run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\ttrue\tfalse'
assert_release_state v1.0.0 $'false\tfalse\ttrue'
assert_pointer_assets_absent
assert_no_mutations

setup_promotable_pair
TEST_GH_CREATE_IMMUTABLE_TAG=updater-stable
assert_fails rejects_immutable_first_pointer run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\ttrue\tfalse'
assert_release_state v1.0.0 $'false\tfalse\ttrue'
assert_pointer_assets_absent

# Rendering/staging the deterministic cask is a pre-authority phase and must fail closed.
setup_promotable_pair
TEST_SHASUM_FAIL=true
assert_fails rejects_unstaged_target_cask run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\ttrue\tfalse'
assert_release_state v1.0.0 $'false\tfalse\ttrue'
assert_no_mutations

setup_promotable_pair
printf 'unexpected' >"${STATE_DIR}/releases/v1.1.0/assets/extra.txt"
assert_fails rejects_bad_source_allowlist run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\ttrue\tfalse'
assert_no_mutations

setup_promotable_pair
printf 'wrong-signature' >"${STATE_DIR}/releases/v1.1.0/assets/RalphX_1.1.0_aarch64.app.tar.gz.sig"
assert_fails rejects_bad_source_signature run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\ttrue\tfalse'
assert_no_mutations
fi

# Public cache verification retries stale bytes without sleeping and fails deterministically when they never converge.
if [[ "${pointer_test_scope}" == "all" || "${pointer_test_scope}" == "stable" || "${pointer_test_scope}" == "stable-cache" ]]; then
setup_promotable_pair
make_release updater-stable true false
make_stable_pointers v1.0.0
mkdir -p "${STATE_DIR}/stale"
cp "${STATE_DIR}/releases/updater-stable/assets/"*.json "${STATE_DIR}/stale/"
TEST_CURL_MODE="stale-once"
TEST_ATTEMPTS="2"
run_control promote v1.1.0 "" ""
assert_pointer_versions v1.1.0
assert_equals 2 "$(cat "${STATE_DIR}/curl-updater-stable-latest-aarch64.json.count")"
assert_equals 2 "$(cat "${STATE_DIR}/curl-updater-stable-latest-x86_64.json.count")"

make_release updater-nightly true false
cp "${STATE_DIR}/releases/updater-stable/assets/"*.json "${STATE_DIR}/releases/updater-nightly/assets/"
run_public_verify updater-nightly "${STATE_DIR}/releases/updater-nightly/assets"
assert_equals 2 "$(cat "${STATE_DIR}/curl-updater-nightly-latest-aarch64.json.count")"
assert_equals 2 "$(cat "${STATE_DIR}/curl-updater-nightly-latest-x86_64.json.count")"

setup_promotable_pair
make_release updater-stable true false
make_stable_pointers v1.0.0
mkdir -p "${STATE_DIR}/stale"
cp "${STATE_DIR}/releases/updater-stable/assets/"*.json "${STATE_DIR}/stale/"
TEST_CURL_MODE="always-stale"
TEST_ATTEMPTS="2"
assert_fails rejects_permanently_stale_public_pointer run_control promote v1.1.0 "" ""
assert_release_state v1.1.0 $'false\tfalse\ttrue'
assert_release_state v1.0.0 $'false\tfalse\tfalse'
assert_pointer_versions v1.1.0
assert_equals 2 "$(cat "${STATE_DIR}/curl-updater-stable-latest-aarch64.json.count")"
fi

# Nightly can advance only from an older complete pointer, repair a candidate/prior split, and rerun exactly.
if [[ "${pointer_test_scope}" == "all" || "${pointer_test_scope}" == "nightly" ]]; then
setup_nightly_pair
run_nightly_control v1.1.0
assert_release_state updater-nightly $'false\ttrue\tfalse'
assert_nightly_pointer_versions v1.1.0

setup_nightly_pair
make_release updater-nightly true false
make_nightly_pointers v1.0.0
run_nightly_control v1.1.0
assert_nightly_pointer_versions v1.1.0
run_nightly_control v1.1.0
assert_nightly_pointer_versions v1.1.0

setup_nightly_pair
make_release updater-nightly true false
make_nightly_pointers v1.0.0
set_nightly_pointer_arch aarch64 v1.1.0
run_nightly_control v1.1.0
assert_nightly_pointer_versions v1.1.0

# Delayed/manual Nightly publishes fail closed instead of moving a newer pointer backward.
setup_nightly_pair
make_release updater-nightly true false
make_nightly_pointers v1.1.0
assert_fails rejects_stale_nightly_candidate run_nightly_control v1.0.0
assert_nightly_pointer_versions v1.1.0
assert_no_mutations

reset_state
make_release v1.0.0 true false
make_source_assets v1.0.0
make_release v1.1.0 true false
make_source_assets v1.1.0
make_release v1.2.0 true false
make_source_assets v1.2.0
make_release updater-nightly true false
make_nightly_pointers v1.0.0
set_nightly_pointer_arch aarch64 v1.1.0
assert_fails rejects_unrelated_nightly_pointer_split run_nightly_control v1.2.0
assert_no_mutations
fi

# Combined-notes promotion: presentation-only overrides on the promoted release and both pointers.
if [[ "${pointer_test_scope}" == "all" || "${pointer_test_scope}" == "stable" || "${pointer_test_scope}" == "stable-core" ]]; then
  # Flag validation fails closed before any gh call. Each case runs against a state where the
  # requested operation would otherwise succeed, so only the guard can explain the rejection.
  write_combined_notes flagcheck
  : >"${TEST_TMP}/flagcheck-empty.md"

  setup_promotable_pair
  assert_fails rejects_pointer_notes_without_body \
    run_control_with_notes promote v1.1.0 "" "" "" "${TEST_TMP}/flagcheck-updater.md"
  assert_no_mutations
  setup_promotable_pair
  assert_fails rejects_body_notes_without_pointer \
    run_control_with_notes promote v1.1.0 "" "" "${TEST_TMP}/flagcheck-body.md" ""
  assert_no_mutations
  setup_promotable_pair
  assert_fails rejects_empty_notes_override \
    run_control_with_notes promote v1.1.0 "" "" "${TEST_TMP}/flagcheck-body.md" "${TEST_TMP}/flagcheck-empty.md"
  assert_no_mutations
  setup_promotable_pair
  assert_fails rejects_missing_notes_override \
    run_control_with_notes promote v1.1.0 "" "" "${TEST_TMP}/flagcheck-body.md" "${TEST_TMP}/absent-notes.md"
  assert_no_mutations

  # Control: this exact promote state succeeds without the notes flags.
  setup_promotable_pair
  run_control promote v1.1.0 "" ""
  assert_release_state v1.1.0 $'false\tfalse\ttrue'

  # Halt rejects the overrides even from a state where halt would otherwise converge.
  setup_haltable_state
  make_release updater-nightly false false
  assert_fails rejects_notes_overrides_on_halt \
    run_control_with_notes halt "" v1.2.0 v1.1.0 "${TEST_TMP}/flagcheck-body.md" "${TEST_TMP}/flagcheck-updater.md"
  assert_no_mutations
  # Control: the same halt state converges once the overrides are dropped.
  run_control halt "" v1.2.0 v1.1.0
  assert_release_state v1.1.0 $'false\tfalse\ttrue'

  # A promotion carrying combined notes rewrites the body, the versioned manifest, and both pointers.
  setup_promotable_pair
  write_combined_notes promote11
  cp "${STATE_DIR}/releases/v1.1.0/assets/latest.json" "${TEST_TMP}/pre-promote-latest-v1.1.0.json"
  run_control_with_notes promote v1.1.0 "" "" \
    "${TEST_TMP}/promote11-body.md" "${TEST_TMP}/promote11-updater.md"
  assert_release_state v1.1.0 $'false\tfalse\ttrue'
  assert_pointer_versions v1.1.0
  assert_release_body v1.1.0 "${TEST_TMP}/promote11-body.md"
  assert_manifest_notes "${STATE_DIR}/releases/v1.1.0/assets/latest.json" "${TEST_TMP}/promote11-updater.md"
  assert_manifest_notes "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json" "${TEST_TMP}/promote11-updater.md"
  assert_manifest_notes "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json" "${TEST_TMP}/promote11-updater.md"
  # Only .notes changed: version, fixed URLs, and signature bytes survive the clobber.
  assert_manifest_identical_except_notes \
    "${TEST_TMP}/pre-promote-latest-v1.1.0.json" "${STATE_DIR}/releases/v1.1.0/assets/latest.json"
  # Presentation must follow proven GitHub authority, never precede it.
  assert_mutation_precedes edit:v1.1.0 body:v1.1.0
  assert_mutation_precedes body:v1.1.0 upload:updater-stable

  # An exact rerun regenerates and reapplies the same presentation idempotently.
  run_control_with_notes promote v1.1.0 "" "" \
    "${TEST_TMP}/promote11-body.md" "${TEST_TMP}/promote11-updater.md"
  assert_release_state v1.1.0 $'false\tfalse\ttrue'
  assert_pointer_versions v1.1.0
  assert_release_body v1.1.0 "${TEST_TMP}/promote11-body.md"
  assert_manifest_notes "${STATE_DIR}/releases/v1.1.0/assets/latest.json" "${TEST_TMP}/promote11-updater.md"

  # Halt after a combined-notes promotion restores from the restore release's own latest.json notes.
  make_release v1.2.0 true false
  make_source_assets v1.2.0
  write_combined_notes promote12
  run_control_with_notes promote v1.2.0 "" "" \
    "${TEST_TMP}/promote12-body.md" "${TEST_TMP}/promote12-updater.md"
  assert_pointer_versions v1.2.0
  assert_manifest_notes "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json" "${TEST_TMP}/promote12-updater.md"
  run_control halt "" v1.2.0 v1.1.0
  assert_release_state v1.2.0 $'false\ttrue\tfalse'
  assert_release_state v1.1.0 $'false\tfalse\ttrue'
  assert_pointer_versions v1.1.0
  # The restored pointers carry v1.1.0's own combined notes, not v1.2.0's and not the stale per-build note.
  assert_manifest_notes_content "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json" "${TEST_TMP}/promote11-updater.md"
  assert_manifest_notes_content "${STATE_DIR}/releases/updater-stable/assets/latest-x86_64.json" "${TEST_TMP}/promote11-updater.md"
  if jq -jr '.notes' "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json" | grep -q 'promote12'; then
    fail "halt left the demoted release's combined notes on the restored pointers"
  fi

  # A published latest.json that diverges from the staged manifest fails closed, before pointers publish.
  setup_promotable_pair
  write_combined_notes corrupt11
  TEST_GH_CORRUPT_UPLOAD_TAG=v1.1.0
  assert_fails rejects_diverged_published_latest_json \
    run_control_with_notes promote v1.1.0 "" "" \
      "${TEST_TMP}/corrupt11-body.md" "${TEST_TMP}/corrupt11-updater.md"
  TEST_GH_CORRUPT_UPLOAD_TAG=""
  assert_pointer_assets_absent

  # Promotion without overrides leaves the body untouched and keeps the per-build manifest notes.
  setup_promotable_pair
  cp "${STATE_DIR}/releases/v1.1.0/assets/latest.json" "${TEST_TMP}/untouched-latest-v1.1.0.json"
  run_control promote v1.1.0 "" ""
  assert_pointer_versions v1.1.0
  [[ ! -f "${STATE_DIR}/releases/v1.1.0/body.md" ]] \
    || fail "promotion without notes overrides rewrote the release body"
  cmp -s "${TEST_TMP}/untouched-latest-v1.1.0.json" "${STATE_DIR}/releases/v1.1.0/assets/latest.json" \
    || fail "promotion without notes overrides replaced the versioned manifest"
  assert_manifest_notes_content "${STATE_DIR}/releases/updater-stable/assets/latest-aarch64.json" \
    "${TEST_TMP}/notes-v1.1.0.md"
fi

echo "PASS: fake-gh/curl release control proves Stable recovery, Nightly monotonicity, fail-closed state classification, and bounded cache retries"
