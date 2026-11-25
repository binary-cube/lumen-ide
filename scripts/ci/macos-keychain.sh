#!/usr/bin/env bash
# The project's signing certificate in a keychain of this run's (CI, macOS), for the app's
# release (release.yml) and the web engine's (web-engine.yml): what's signed with it is known
# to macOS from one version to the next.
#
# Reads MACOS_SIGNING_P12 (the certificate, base64) and MACOS_SIGNING_PASSWORD (its
# password); RUNNER_TEMP and GITHUB_ENV as GitHub Actions sets them. Writes
# MACOS_SIGN_IDENTITY to the job's environment, for the signing steps after it. The
# keychain's own password is made here and never leaves the run.
set -euo pipefail

: "${MACOS_SIGNING_P12:?the certificate (base64) is missing}"
: "${MACOS_SIGNING_PASSWORD:?the password of the certificate is missing}"
: "${RUNNER_TEMP:?not in a GitHub Actions run}"
: "${GITHUB_ENV:?not in a GitHub Actions run}"

# The certificate's common name: what `codesign --sign` is given.
identity="Binary Cube Code Signing"

keychain="$RUNNER_TEMP/signing.keychain-db"
certificate="$RUNNER_TEMP/signing.p12"
password="$(openssl rand -hex 16)"
trap 'rm -f "$certificate"' EXIT

echo "$MACOS_SIGNING_P12" | base64 --decode > "$certificate"
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$certificate" -k "$keychain" -P "$MACOS_SIGNING_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
# The run's keychain first, the user's own after it (the system's lookups still work).
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
echo "MACOS_SIGN_IDENTITY=$identity" >> "$GITHUB_ENV"
