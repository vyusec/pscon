#!/usr/bin/env bash
set -euo pipefail

# Install the pscon binary into ~/.local/bin (or PSCON_BIN_DIR).
# JSner is installed separately from https://github.com/vyusec/JSner.

ROOT="$(cd "$(dirname "$0")" && pwd)"
if [[ ! -f "$ROOT/pscon" && -f "$ROOT/../pscon" ]]; then
  ROOT="$(cd "$ROOT/.." && pwd)"
fi
BIN_DIR="${PSCON_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$BIN_DIR"
install -m 0755 "$ROOT/pscon" "$BIN_DIR/pscon"

echo "Installed pscon to $BIN_DIR/pscon"
echo
echo "Extension:"
echo "  Download or clone https://github.com/vyusec/JSner"
echo "  Open chrome://extensions and enable Developer mode."
echo "  Load unpacked: select the JSner folder containing manifest.json."
echo
echo "Start the TUI and bridge:"
echo "  pscon tui"
echo
echo "Get the bridge token for the extension popup:"
echo "  pscon bridge token"
