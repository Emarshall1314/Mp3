#!/usr/bin/env bash
# Installs everything a Linux (Debian/Ubuntu) machine needs to build, test and package Cadenza.
# Used by CI and for fresh development machines. Run as root (or via sudo).
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
# A stalled mirror connection is retried rather than left to hang the CI job.
# Waiting for another apt (unattended upgrades on a fresh machine) is bounded too, and each step
# has a time limit, so a hang ends as an error that says where it was.
APT=(-o Acquire::Retries=5 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20
  -o DPkg::Lock::Timeout=300)
echo "apt: update"
timeout 600 apt-get "${APT[@]}" update -q
echo "apt: install"
timeout 1200 apt-get "${APT[@]}" install -y -q --no-install-recommends \
  build-essential pkg-config cmake git python3 python3-venv \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev libssl-dev \
  libasound2-dev \
  ffmpeg sox libsox-fmt-all flac opus-tools vorbis-tools lame wavpack \
  xvfb alsa-utils file
# Monkey's Audio (APE fixtures) has no package; `cargo xtask fixtures` builds it from source.
