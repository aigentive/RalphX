#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

disable_unneeded_microsoft_apt_sources() {
  local disabled_dir="/tmp/ralphx-disabled-apt-sources"
  local source

  sudo mkdir -p "${disabled_dir}"

  shopt -s nullglob
  for source in /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
    if sudo grep -Eq 'packages\.microsoft\.com/(repos/azure-cli|ubuntu/24\.04/prod)' "${source}"; then
      sudo mv "${source}" "${disabled_dir}/$(basename "${source}").disabled"
    fi
  done
  shopt -u nullglob
}

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

disable_unneeded_microsoft_apt_sources
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
