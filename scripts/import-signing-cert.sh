#!/usr/bin/env bash
# Import the self-signed code signing certificate for the macOS release build.
#
# macOS keeps permissions such as Screen Recording for the app's designated
# requirement. An ad-hoc signature makes that requirement the code hash, which
# changes with every build, so each update lost the permission. Signing with one
# fixed certificate makes it "identifier … and certificate root = H…", the same
# for every release.
#
# Tauri's own APPLE_CERTIFICATE import only accepts Apple-issued certificates, so
# this imports the p12 into a keychain on the search list and points Tauri at it
# with APPLE_SIGNING_IDENTITY. An untrusted self-signed certificate signs fine
# from there; codesign only refuses it when the keychain is not searched.
#
# Env: MACOS_CERTIFICATE (base64 p12), MACOS_CERTIFICATE_PASSWORD,
#      SIGNING_IDENTITY (certificate common name). Without the certificate the
#      build stays ad-hoc signed.
set -euo pipefail

if [ -z "${MACOS_CERTIFICATE:-}" ]; then
  echo "MACOS_CERTIFICATE not set; the app stays ad-hoc signed"
  exit 0
fi
: "${SIGNING_IDENTITY:?}" "${MACOS_CERTIFICATE_PASSWORD:?}"

keychain="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/latexsnipper-signing.keychain-db"
password=$(openssl rand -hex 16)
p12=$(mktemp)
trap 'rm -f "$p12"' EXIT
printf '%s' "$MACOS_CERTIFICATE" | base64 --decode > "$p12"

security create-keychain -p "$password" "$keychain"
security set-keychain-settings -t 3600 -u "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$p12" -f pkcs12 -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
# codesign finds identities only in keychains on the user search list.
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
security find-certificate -c "$SIGNING_IDENTITY" "$keychain" >/dev/null

echo "APPLE_SIGNING_IDENTITY=$SIGNING_IDENTITY" >> "${GITHUB_ENV:-/dev/null}"
echo "signing with \"$SIGNING_IDENTITY\""
