#!/bin/sh
# macOS: mount the built DMG, check it links only system libraries, then run
# the cross-platform smoke test (scripts/smoke.sh) against the app inside it.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DMG=$(ls "$ROOT"/backend/target/release/bundle/dmg/LaTeXSnipper_*.dmg | head -1)
MNT=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$DMG" >/dev/null
trap 'hdiutil detach "$MNT" >/dev/null 2>&1 || true' EXIT
BIN="$MNT/LaTeXSnipper.app/Contents/MacOS/latexsnipper"

echo "== [macos] linkage (system libraries only)"
if otool -L "$BIN" | tail -n +2 | awk '{print $1}' | grep -vE '^/(usr/lib|System)/'; then
  echo "FAIL: non-system dylib"; exit 1
fi
echo "== [macos] signature covers the whole bundle"
codesign --verify --deep --strict "$MNT/LaTeXSnipper.app"
echo ok

"$ROOT/scripts/smoke.sh" "$BIN" "$@"
