#!/usr/bin/env bash
set -euo pipefail

# Laya currently requires Python 3.10-3.13. Python 3.14 breaks torch.compile in
# the published Laya release, so this script defaults to python3.12.
PYTHON="${PYTHON:-python3.12}"
VENV="${PSCON_LAYA_VENV:-$HOME/.local/share/pscon/laya-venv}"

echo "[pscon] creating Laya venv at $VENV with $PYTHON"
"$PYTHON" -m venv "$VENV"
"$VENV/bin/pip" install -U pip
"$VENV/bin/pip" install -U 'laya[serve]'

echo "[pscon] Laya installed. Start it with:"
echo "  LAYA_HOST=127.0.0.1 LAYA_PORT=8766 $VENV/bin/laya-serve"
echo "or let pscon autostart it from the [decisions.laya] config section."
