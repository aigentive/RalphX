#!/usr/bin/env bash
set -euo pipefail

profile="${1:-release}"
case "$profile" in
  dev) output_dir="debug" ;;
  release) output_dir="release" ;;
  *) echo "usage: $0 [dev|release]" >&2; exit 2 ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
cd "${tauri_dir}"

target_triple="${TAURI_ENV_TARGET_TRIPLE:-}"
if [[ -z "${target_triple}" ]]; then
  target_triple="$(rustc -vV | sed -n 's/^host: //p')"
fi
test -n "$target_triple"

cargo_args=(build -p ralphx-workflow-runner)
if [[ "$profile" == "release" ]]; then
  cargo_args+=(--release)
fi
cargo_args+=(--target "${target_triple}")
cargo "${cargo_args[@]}"

mkdir -p binaries
cp "${CARGO_TARGET_DIR:-target}/${target_triple}/${output_dir}/ralphx-workflow-runner" \
  "binaries/ralphx-workflow-runner-${target_triple}"
