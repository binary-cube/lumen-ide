#!/usr/bin/env bash
# The rolling `web-engine` release's archives no longer named by its list deleted (CI:
# web-engine.yml, after the new list is uploaded). Only `web-engine-*.tar.xz` assets are
# touched; the list and its signature always stay.
#
#   web-engine-prune.sh <owner/repo> <web-engine.json>
#
# Run after the list is uploaded, never before: a client reading the old list a moment
# earlier still finds every archive it names until then.
set -euo pipefail

repo="${1:?owner/repo}"
list="${2:?the list just published}"
release="web-engine"

# The archives the list names: the last part of each package's address.
named="$(jq -r '.packages[].url | split("/") | last' "$list")"
test -n "$named" || { echo "::error::$list names no package: nothing pruned" >&2; exit 1; }

assets="$(gh release view "$release" --repo "$repo" --json assets --jq '.assets[].name')"
while IFS= read -r asset; do
  case "$asset" in
    web-engine-*.tar.xz) ;;
    *) continue ;;
  esac
  if ! grep -Fxq "$asset" <<<"$named"; then
    echo "pruned: $asset"
    gh release delete-asset "$release" "$asset" --repo "$repo" --yes
  fi
done <<<"$assets"
