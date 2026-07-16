#!/usr/bin/env bash
set -euo pipefail

profile="${1:-release}"
case "$profile" in
  dev) output_dir="debug" ;;
  release) output_dir="release" ;;
  *) echo "usage: $0 [dev|release]" >&2; exit 2 ;;
esac

target_triple="$(rustc -vV | sed -n 's/^host: //p')"
test -n "$target_triple"
if [[ "$profile" == "release" ]]; then
  cargo build -p ralphx-workflow-runner --release
else
  cargo build -p ralphx-workflow-runner
fi
mkdir -p binaries
cp "${CARGO_TARGET_DIR:-target}/${output_dir}/ralphx-workflow-runner" \
  "binaries/ralphx-workflow-runner-${target_triple}"
