#!/usr/bin/env bash
# Packaged-app smoke test for macOS, Linux and Windows (Git Bash):
#   scripts/smoke.sh <app binary> [--require-models]
# 1. real OCR through the shipped binary (when the MathCraft models are available),
# 2. a stable error when models are missing,
# 3. the app boots headless: no window, Dock/tray icon or hotkey; the bundled UI
#    renders a formula with MathJax, reports over IPC, and the app exits.
set -euo pipefail
BIN=${1:?usage: smoke.sh <app binary> [--require-models]}
REQUIRE_MODELS=${2:-}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
FIX="$ROOT/backend/tests/fixtures"
case "$(uname -s)" in
  Darwin) OS=macos; DEFAULT_MODELS="$HOME/Library/Application Support/LaTeXSnipper/MathCraft/models" ;;
  MINGW* | MSYS* | CYGWIN*) OS=windows; DEFAULT_MODELS="${LOCALAPPDATA:-}/LaTeXSnipper Lite/MathCraft/models" ;;
  *) OS=linux; DEFAULT_MODELS="${XDG_DATA_HOME:-$HOME/.local/share}/latexsnipper-lite/MathCraft/models" ;;
esac
MODELS=${LATEXSNIPPER_MODELS:-$DEFAULT_MODELS}
tmpdir() { mktemp -d 2>/dev/null || mktemp -d -t latexsnipper; }

echo "== [$OS] headless recognition"
if [ -f "$MODELS/mathcraft-formula-rec/encoder_model.onnx" ]; then
  DATA=$(tmpdir); mkdir -p "$DATA/MathCraft"
  if [ "$OS" = windows ]; then cp -R "$MODELS" "$DATA/MathCraft/models"; else ln -s "$MODELS" "$DATA/MathCraft/models"; fi
  OUT=$(LATEXSNIPPER_DATA_DIR="$DATA" "$BIN" --selftest "$FIX/frac.png" | tr -d '\r')
  EXPECTED=$(cd "$FIX" && node -p "require('./python_reference.json').frac.text")
  [ "$OUT" = "$EXPECTED" ] || { printf "FAIL: got %s\n  want %s\n" "$OUT" "$EXPECTED"; exit 1; }
  printf "ok: %s\n" "$OUT"
elif [ "$REQUIRE_MODELS" = --require-models ]; then
  echo "FAIL: models not found under $MODELS"; exit 1
else
  echo "skip: models not installed"
fi

echo "== [$OS] missing models report a stable error"
# Capture first: with pipefail, `| grep -q` would report the SIGPIPE it causes.
ERR=$(LATEXSNIPPER_DATA_DIR=$(tmpdir) "$BIN" --selftest "$FIX/frac.png" 2>&1 || true)
case "$ERR" in *MODELS_MISSING*) echo ok ;; *) printf "FAIL: %s\n" "$ERR"; exit 1 ;; esac

echo "== [$OS] app boots headless and its UI passes a self-check"
RUN=()
if [ "$OS" = linux ] && [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then RUN=(xvfb-run -a); fi
LOG=$(mktemp)
LATEXSNIPPER_HEADLESS=1 LATEXSNIPPER_DATA_DIR=$(tmpdir) ${RUN[@]+"${RUN[@]}"} "$BIN" >"$LOG" 2>&1 &
PID=$!
i=0
while kill -0 "$PID" 2>/dev/null && [ $i -lt 600 ]; do sleep 0.1; i=$((i + 1)); done
if kill -0 "$PID" 2>/dev/null; then
  kill "$PID"; echo "FAIL: no self-check answer within 60s"; cat "$LOG"; exit 1
fi
wait "$PID" && CODE=0 || CODE=$?
if ! grep -q "LATEXSNIPPER_READY ok=true" "$LOG" || [ "$CODE" -ne 0 ]; then
  echo "FAIL: exit $CODE"; cat "$LOG"; exit 1
fi
printf "ok: %s\n" "$(grep LATEXSNIPPER_READY "$LOG" | tr -d '\r')"
echo "PASS"
