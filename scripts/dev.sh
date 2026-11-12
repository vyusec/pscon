#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export PSCON_HOME="${PSCON_HOME:-/tmp/opencode/pscon-dev}"
export PSCON_CONFIG="${PSCON_CONFIG:-$PSCON_HOME/config.toml}"
cargo +1.98.0 run -p pscon -- "$@"
