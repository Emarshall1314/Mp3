#!/usr/bin/env bash
# Installs everything a Linux (Debian/Ubuntu) machine needs to build, test and package Cadenza.
# Used by CI and for fresh development machines. Run as root (or via sudo).
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  build-essential pkg-config cmake git python3 python3-venv \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev libssl-dev \
  libasound2-dev libopus-dev libwavpack-dev \
  ffmpeg sox libsox-fmt-all flac opus-tools vorbis-tools lame wavpack \
  xvfb alsa-utils file
# Monkey's Audio (APE fixtures) has no package; `cargo xtask fixtures` builds it from source.
