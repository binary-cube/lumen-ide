#!/usr/bin/env bash
# The web engine's published list (`web-engine.json` and its signature) downloaded from the
# rolling `web-engine` release into a folder (CI: web-engine.yml's draft, publish and relist).
#
#   web-engine-published.sh <owner/repo> <dir> [--required]
#
# Prints the list's path when there is one, nothing when none was published yet: no
# `web-engine` release, or one without a list. Anything else fails: a list named but not
# downloaded, a list without its signature, an error asking GitHub. A list silently missing
# would let a draft drop the other protocols' packages, or a publish skip its guard against
# dropping what was published since. `--required`: no published list fails too (a relist).
set -euo pipefail

repo="${1:?owner/repo}"
dir="${2:?the folder to download into}"
required="${3:-}"
release="web-engine"
list="web-engine.json"

mkdir -p "$dir"

# The release's assets; a release not found is the one answer that means "nothing yet".
if ! assets="$(gh release view "$release" --repo "$repo" --json assets --jq '.assets[].name' 2>"$dir/.error")"; then
  if grep -qi "release not found" "$dir/.error"; then
    rm -f "$dir/.error"
    if [ "$required" = "--required" ]; then
      echo "::error::no $release release: nothing published to list again" >&2
      exit 1
    fi
    exit 0
  fi
  cat "$dir/.error" >&2
  rm -f "$dir/.error"
  echo "::error::the $release release could not be read" >&2
  exit 1
fi
rm -f "$dir/.error"

listed() { grep -Fxq "$1" <<<"$assets"; }

if ! listed "$list"; then
  if [ "$required" = "--required" ]; then
    echo "::error::the $release release has no $list" >&2
    exit 1
  fi
  exit 0
fi
listed "$list.sig" || { echo "::error::$list is published without its signature" >&2; exit 1; }

# Its own output to the log, never into the path printed below.
gh release download "$release" --repo "$repo" --dir "$dir" --clobber \
  --pattern "$list" --pattern "$list.sig" >&2
test -s "$dir/$list" || { echo "::error::$list not downloaded" >&2; exit 1; }
test -s "$dir/$list.sig" || { echo "::error::$list.sig not downloaded" >&2; exit 1; }
echo "$dir/$list"
