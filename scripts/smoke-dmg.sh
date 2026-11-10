#!/bin/sh
# Packaged-artifact E2E: mount the DMG, check linkage, run headless OCR, launch the GUI.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DMG=$(ls "$ROOT"/src-tauri/target/release/bundle/dmg/LaTeXSnipper_*.dmg | head -1)
FIX="$ROOT/src-tauri/tests/fixtures"
MNT=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$DMG" >/dev/null
trap 'hdiutil detach "$MNT" >/dev/null 2>&1 || true' EXIT
BIN="$MNT/LaTeXSnipper.app/Contents/MacOS/latexsnipper"

echo "== linkage (system libraries only)"
if otool -L "$BIN" | tail -n +2 | awk '{print $1}' | grep -vE '^/(usr/lib|System)/'; then
  echo "FAIL: non-system dylib"; exit 1
fi

echo "== headless recognition"
if [ -n "$(ls "${LATEXSNIPPER_MODELS:-$HOME/Library/Application Support/LaTeXSnipper/MathCraft/models}" 2>/dev/null)" ]; then
  DATA=$(mktemp -d); mkdir -p "$DATA/MathCraft"
  ln -s "${LATEXSNIPPER_MODELS:-$HOME/Library/Application Support/LaTeXSnipper/MathCraft/models}" "$DATA/MathCraft/models"
  OUT=$(LATEXSNIPPER_DATA_DIR="$DATA" "$BIN" --selftest "$FIX/frac.png")
  EXPECTED=$(python3 -c "import json;print(json.load(open('$FIX/python_reference.json'))['frac']['text'])")
  [ "$OUT" = "$EXPECTED" ] || { echo "FAIL: got $OUT"; exit 1; }
  printf "ok: %s\n" "$OUT"
else
  echo "skip: models not installed"
fi

echo "== missing models report a stable error"
if LATEXSNIPPER_DATA_DIR=$(mktemp -d) "$BIN" --selftest "$FIX/frac.png" 2>&1 | grep -q MODELS_MISSING; then echo ok; else echo FAIL; exit 1; fi

echo "== GUI launches and stays up"
LATEXSNIPPER_DATA_DIR=$(mktemp -d) "$BIN" >/dev/null 2>&1 &
PID=$!
sleep 5
kill -0 "$PID" || { echo "FAIL: GUI exited"; exit 1; }
kill "$PID"; wait "$PID" 2>/dev/null || true
echo "ok"
echo "PASS"
