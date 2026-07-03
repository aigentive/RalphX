#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if REPO_ROOT="$(git -C "${SCRIPT_DIR}/.." rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
fi

MANIFEST_PATH="${REPO_ROOT}/src-tauri/Cargo.toml"
OUTPUT_ROOT="${REPO_ROOT}/.artifacts/rust-build-bench"
DEFAULT_CASES=(dev-build lib-test-no-run release-build)
LABEL="local"
RUNS=3
CASES=("${DEFAULT_CASES[@]}")

usage() {
  cat <<'EOF'
Usage: scripts/bench-rust-build.sh [--label <name>] [--runs <count>] [--cases <list>]

Measures Rust build cost with isolated Cargo target dirs and writes ignored
artifacts under .artifacts/rust-build-bench/<timestamp>-<label>/.

Options:
  --label <name>   Safe label for the output directory. Default: local
  --runs <count>   Timed runs per case/phase. Default: 3
  --cases <list>   Space or comma separated cases. Default:
                   dev-build lib-test-no-run release-build
  -h, --help       Show this help

Cases:
  dev-build        cargo build --manifest-path src-tauri/Cargo.toml
  lib-test-no-run  cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run
  release-build    cargo build --manifest-path src-tauri/Cargo.toml --release
  tauri-no-bundle  CI=false npm run tauri -- build --no-bundle from frontend/

Typical PR 0.1 usage:
  scripts/bench-rust-build.sh --label before
  scripts/bench-rust-build.sh --label after

Use --runs 1 for a syntax/smoke check.
EOF
}

die() {
  printf '[bench-rust-build] %s\n' "$*" >&2
  exit 1
}

log() {
  printf '[bench-rust-build] %s\n' "$*"
}

parse_cases() {
  local raw="$1"
  local normalized
  normalized="${raw//,/ }"
  # shellcheck disable=SC2206
  CASES=(${normalized})
}

validate_label() {
  if [[ ! "${LABEL}" =~ ^[A-Za-z0-9._-]+$ ]]; then
    die "label must contain only letters, numbers, dot, underscore, or dash"
  fi
}

validate_runs() {
  if [[ ! "${RUNS}" =~ ^[0-9]+$ || "${RUNS}" -lt 1 ]]; then
    die "--runs must be a positive integer"
  fi
}

validate_cases() {
  local case_name
  for case_name in "${CASES[@]}"; do
    case "${case_name}" in
      dev-build|lib-test-no-run|release-build|tauri-no-bundle)
        ;;
      *)
        die "unknown case: ${case_name}"
        ;;
    esac
  done
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --label)
      [[ "$#" -ge 2 ]] || die "--label requires a value"
      LABEL="$2"
      shift 2
      ;;
    --runs)
      [[ "$#" -ge 2 ]] || die "--runs requires a value"
      RUNS="$2"
      shift 2
      ;;
    --cases)
      [[ "$#" -ge 2 ]] || die "--cases requires a value"
      parse_cases "$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

validate_label
validate_runs
validate_cases

TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${OUTPUT_ROOT}/${TIMESTAMP}-${LABEL}"
RAW_DIR="${RUN_DIR}/raw"
TIME_DIR="${RUN_DIR}/time"
TARGETS_DIR="${RUN_DIR}/targets"
SUMMARY_MD="${RUN_DIR}/summary.md"
RESULTS_TSV="${RUN_DIR}/results.tsv"
ARTIFACTS_TSV="${RUN_DIR}/artifacts.tsv"
ENV_TXT="${RUN_DIR}/env.txt"

mkdir -p "${RAW_DIR}" "${TIME_DIR}" "${TARGETS_DIR}"

write_env_snapshot() {
  {
    printf 'repo_root=%s\n' "${REPO_ROOT}"
    printf 'git_sha=%s\n' "$(git -C "${REPO_ROOT}" rev-parse HEAD 2>/dev/null || true)"
    printf 'host=%s\n' "$(uname -a)"
    printf 'cargo=%s\n' "$(cargo --version 2>/dev/null || true)"
    printf 'rustc=%s\n' "$(rustc --version 2>/dev/null || true)"
    printf 'node=%s\n' "$(node --version 2>/dev/null || true)"
    printf 'npm=%s\n' "$(npm --version 2>/dev/null || true)"
    printf 'RUSTFLAGS=%s\n' "${RUSTFLAGS:-}"
    printf 'CARGO_BUILD_RUSTFLAGS=%s\n' "${CARGO_BUILD_RUSTFLAGS:-}"
    printf 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=%s\n' "${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-}"
    printf '\n[lib/profile snippets]\n'
    sed -n '/^\[lib\]/,/^\[build-dependencies\]/p;/^\[profile\./,$p' "${MANIFEST_PATH}"
  } > "${ENV_TXT}"
}

run_case_command() {
  local case_name="$1"
  local target_dir="$2"

  case "${case_name}" in
    dev-build)
      CARGO_TARGET_DIR="${target_dir}" cargo build --manifest-path "${MANIFEST_PATH}"
      ;;
    lib-test-no-run)
      CARGO_TARGET_DIR="${target_dir}" cargo test --manifest-path "${MANIFEST_PATH}" --lib --no-run
      ;;
    release-build)
      CARGO_TARGET_DIR="${target_dir}" cargo build --manifest-path "${MANIFEST_PATH}" --release
      ;;
    tauri-no-bundle)
      (
        cd "${REPO_ROOT}/frontend"
        CI=false CARGO_TARGET_DIR="${target_dir}" npm run tauri -- build --no-bundle
      )
      ;;
  esac
}

export REPO_ROOT MANIFEST_PATH
export -f run_case_command

time_command() {
  local time_file="$1"
  local raw_file="$2"
  shift 2

  if /usr/bin/time -f 'wall_seconds=%e' true >/dev/null 2>&1; then
    /usr/bin/time -f $'wall_seconds=%e\nuser_seconds=%U\nsys_seconds=%S\nmax_rss_kb=%M' -o "${time_file}" \
      bash -c 'raw="$1"; shift; "$@" >"${raw}" 2>&1' bash "${raw_file}" "$@"
  else
    /usr/bin/time -p \
      bash -c 'raw="$1"; shift; "$@" >"${raw}" 2>&1' bash "${raw_file}" "$@" \
      2> "${time_file}"
  fi
}

record_artifacts() {
  local case_name="$1"
  local phase="$2"
  local run_number="$3"
  local target_dir="$4"
  local target_kb=0

  if [[ -d "${target_dir}" ]]; then
    target_kb="$(du -sk "${target_dir}" | awk '{print $1}')"
  fi

  printf '%s\t%s\t%s\ttarget_dir\t%s\t%s\n' \
    "${case_name}" "${phase}" "${run_number}" "${target_kb}" "${target_dir#${REPO_ROOT}/}" >> "${ARTIFACTS_TSV}"

  find "${target_dir}" -type f \( \
    -name 'ralphx' -o \
    -name 'ralphx.exe' -o \
    -name 'libralphx_lib.rlib' -o \
    -name 'libralphx_lib.a' -o \
    -name 'libralphx_lib.dylib' \
  \) -print0 2>/dev/null | while IFS= read -r -d '' artifact; do
    local artifact_kb
    artifact_kb="$(du -sk "${artifact}" | awk '{print $1}')"
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
      "${case_name}" "${phase}" "${run_number}" "artifact" "${artifact_kb}" "${artifact#${REPO_ROOT}/}" >> "${ARTIFACTS_TSV}"
  done
}

append_result() {
  local case_name="$1"
  local phase="$2"
  local run_number="$3"
  local time_file="$4"
  local wall=""
  local user=""
  local sys=""
  local rss=""

  while IFS='= ' read -r key value; do
    case "${key}" in
      wall_seconds|real) wall="${value}" ;;
      user_seconds|user) user="${value}" ;;
      sys_seconds|sys) sys="${value}" ;;
      max_rss_kb) rss="${value}" ;;
    esac
  done < "${time_file}"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${case_name}" "${phase}" "${run_number}" "${wall}" "${user}" "${sys}" "${rss}" >> "${RESULTS_TSV}"
}

median_for() {
  local case_name="$1"
  local phase="$2"
  awk -F '\t' -v c="${case_name}" -v p="${phase}" '
    $1 == c && $2 == p && $4 != "" { values[++n] = $4 + 0 }
    END {
      if (n == 0) {
        print "n/a"
        exit
      }
      for (i = 1; i <= n; i++) {
        for (j = i + 1; j <= n; j++) {
          if (values[j] < values[i]) {
            tmp = values[i]; values[i] = values[j]; values[j] = tmp
          }
        }
      }
      mid = int((n + 1) / 2)
      if (n % 2 == 1) {
        printf "%.3f", values[mid]
      } else {
        printf "%.3f", (values[mid] + values[mid + 1]) / 2
      }
    }
  ' "${RESULTS_TSV}"
}

write_summary() {
  local case_name
  {
    printf '# Rust Build Benchmark: %s\n\n' "${LABEL}"
    printf '- Run directory: `%s`\n' "${RUN_DIR#${REPO_ROOT}/}"
    printf '- Runs per case/phase: `%s`\n\n' "${RUNS}"
    printf '| Case | Cold median wall (s) | Warm root rebuild median wall (s) |\n'
    printf '|---|---:|---:|\n'
    for case_name in "${CASES[@]}"; do
      printf '| `%s` | %s | %s |\n' \
        "${case_name}" \
        "$(median_for "${case_name}" cold)" \
        "$(median_for "${case_name}" warm-root-rebuild)"
    done
    printf '\nRaw logs, timing files, environment snapshot, and artifact sizes are in this ignored directory.\n'
  } > "${SUMMARY_MD}"
}

benchmark_case() {
  local case_name="$1"
  local phase
  local run_number
  local target_dir
  local raw_file
  local time_file

  for phase in cold warm-root-rebuild; do
    target_dir="${TARGETS_DIR}/${case_name}-${phase}"
    mkdir -p "${target_dir}"

    if [[ "${phase}" == "warm-root-rebuild" ]]; then
      log "${case_name}: warming dependencies"
      run_case_command "${case_name}" "${target_dir}" > "${RAW_DIR}/${case_name}-${phase}-warmup.log" 2>&1
      CARGO_TARGET_DIR="${target_dir}" cargo clean --manifest-path "${MANIFEST_PATH}" -p ralphx \
        > "${RAW_DIR}/${case_name}-${phase}-clean.log" 2>&1
    fi

    for ((run_number = 1; run_number <= RUNS; run_number++)); do
      raw_file="${RAW_DIR}/${case_name}-${phase}-run${run_number}.log"
      time_file="${TIME_DIR}/${case_name}-${phase}-run${run_number}.txt"
      log "${case_name}: ${phase} run ${run_number}/${RUNS}"
      time_command "${time_file}" "${raw_file}" run_case_command "${case_name}" "${target_dir}"
      append_result "${case_name}" "${phase}" "${run_number}" "${time_file}"
      record_artifacts "${case_name}" "${phase}" "${run_number}" "${target_dir}"
    done
  done
}

write_env_snapshot
printf 'case\tphase\trun\twall_seconds\tuser_seconds\tsys_seconds\tmax_rss_kb\n' > "${RESULTS_TSV}"
printf 'case\tphase\trun\tkind\tkb\tpath\n' > "${ARTIFACTS_TSV}"

for case_name in "${CASES[@]}"; do
  benchmark_case "${case_name}"
done

write_summary
log "summary: ${SUMMARY_MD#${REPO_ROOT}/}"
