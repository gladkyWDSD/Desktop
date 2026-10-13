#!/usr/bin/env bash
# Standalone checkout: build only, never replace an installed desktop.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
cargo build --release --locked
printf '%s\n' 'Built Desktop Workspace. Run ./bin/desktop-workspace start.'
