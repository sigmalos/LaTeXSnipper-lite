#!/bin/sh
# Packaged-artifact E2E: mount the DMG, check linkage, run headless OCR, launch the GUI.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DMG=$(ls "$ROOT"/backend/target/release/bundle/dmg/LaTeXSnipper_*.dmg | head -1)
FIX="$ROOT/backend/tests/fixtures"
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

echo "== app boots headless and its UI passes a self-check"
# LATEXSNIPPER_HEADLESS=1: no window, Dock icon, menu bar item or global hotkey,
# so this never touches the desktop it runs on. The bundled UI renders a formula
# with MathJax, reports over IPC, and the app exits with the result.
LOG=$(mktemp)
LATEXSNIPPER_HEADLESS=1 LATEXSNIPPER_DATA_DIR=$(mktemp -d) "$BIN" >"$LOG" 2>&1 &
PID=$!
i=0
while kill -0 "$PID" 2>/dev/null && [ $i -lt 300 ]; do sleep 0.1; i=$((i + 1)); done
if kill -0 "$PID" 2>/dev/null; then
  kill "$PID"; echo "FAIL: no self-check answer within 30s"; cat "$LOG"; exit 1
fi
wait "$PID" && CODE=0 || CODE=$?
grep -q "LATEXSNIPPER_READY ok=true" "$LOG" && [ "$CODE" -eq 0 ] || { echo "FAIL: exit $CODE"; cat "$LOG"; exit 1; }
printf "ok: %s\n" "$(grep LATEXSNIPPER_READY "$LOG")"
echo "PASS"
