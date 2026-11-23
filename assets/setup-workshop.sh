#!/bin/sh
set -eu
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  ca-certificates curl git ripgrep unzip
npm install --global --prefix "$HOME/.local" bun@{{BUN_VERSION}}
BUN_INSTALL_GLOBAL_DIR="$HOME/.local/share/omp" BUN_INSTALL_BIN="$HOME/.local/bin" \
  "$HOME/.local/bin/bun" install --global @oh-my-pi/pi-coding-agent@{{OMP_VERSION}}
mkdir -p "$HOME/.config" "$HOME/.omp/agent" "$HOME/src"
"$HOME/.local/bin/omp" --version
