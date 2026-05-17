#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

retry_apt() {
  local attempt
  for attempt in 1 2 3; do
    if sudo apt-get \
      -o Acquire::Retries=3 \
      -o Acquire::http::Timeout=30 \
      -o Acquire::https::Timeout=30 \
      "$@"; then
      return 0
    fi

    if [[ "${attempt}" -eq 3 ]]; then
      return 1
    fi

    sleep $((attempt * 10))
  done
}

retry_apt update
retry_apt install --no-install-recommends -y \
  build-essential \
  curl \
  file \
  lsof \
  libayatana-appindicator3-dev \
  libgtk-3-dev \
  librsvg2-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  pkg-config \
  wget
