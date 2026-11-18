#!/usr/bin/env bash
# Keeps target/ small without building from scratch: cargo never removes what older builds
# left (every feature set a `-p` run unified differently, every test binary renamed, a
# build script run again…), so what the checks no longer use goes — what they use stays.
#
#   ./scripts/sweep.sh            before a check (check.sh): what the last full check didn't
#                                 use, untouched since it began, removed; this check begun
#   ./scripts/sweep.sh --record   after a full check passed: what it used kept as the list
#   ./scripts/sweep.sh --now      the builds emptied (as above the limit)
#
# By hand: scripts/clean.sh (the same, or everything).
#
# EDITOR_SWEEP_DRY=1: what would go is listed, nothing removed.
#
# What a check used: the hashes of every unit cargo built or found fresh (its JSON messages,
# saved by check.sh under target/.sweep/run). In each profile directory of the checks
# (target/debug, target/check-host/debug, target/check-windows/{,x86_64-pc-windows-gnu/}debug):
#
# - deps/, .fingerprint/, build/: an entry whose hash the last full check didn't use and
#   that nothing touched since it began (your own `cargo test -p …` since then stays);
# - incremental/: a crate's cache untouched since the last full check began and for 3 hours,
#   and whose objects no unit of that check links (macOS; elsewhere, one not built lately
#   compiles once without it when it's changed); none at all where the checks build without
#   them (check-host, check-windows: CARGO_INCREMENTAL=0);
# - deps/*.rcgu.o (macOS keeps a unit's object files beside it for its debug information,
#   under new names at every build): those no longer in a cache kept — an older build's.
#
# target/release and target/x86_64-pc-windows-gnu (build.sh's) are never touched. Past
# EDITOR_TARGET_LIMIT_GB (default 40) of the checks' builds, they're emptied as a fallback.
#
# It also keeps target/ out of Spotlight and Time Machine (macOS): millions of build files
# indexed and backed up for nothing slow everything down.

set -uo pipefail
# Plain bytes for sort, comm and awk (the same order everywhere, and faster).
export LC_ALL=C

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"
SWEEP="$TARGET/.sweep"
LIMIT_GB="${EDITOR_TARGET_LIMIT_GB:-40}"
# How long an incremental cache outlives the last full check that didn't touch it.
KEEP_INCREMENTAL_MIN="${EDITOR_SWEEP_INCREMENTAL_MIN:-180}"

# The checks' own builds (never build.sh's).
BUILDS=("$TARGET/debug" "$TARGET/check-host" "$TARGET/check-windows")
PROFILES=("$TARGET/debug" "$TARGET/check-host/debug" "$TARGET/check-windows/debug"
    "$TARGET/check-windows/x86_64-pc-windows-gnu/debug")
# Built without incremental compilation (check.sh): their caches are never read again.
NO_INCREMENTAL=("$TARGET/check-host/debug" "$TARGET/check-windows/debug"
    "$TARGET/check-windows/x86_64-pc-windows-gnu/debug")

mkdir -p "$SWEEP"
if [ "$(uname)" = "Darwin" ]; then
    touch "$TARGET/.metadata_never_index"
    # Asked once (tmutil takes a while): the marker goes with target/.
    if [ ! -f "$TARGET/.time-machine-excluded" ]; then
        tmutil isexcluded "$TARGET" 2>/dev/null | grep -q Excluded \
            || tmutil addexclusion "$TARGET" 2>/dev/null || true
        touch "$TARGET/.time-machine-excluded"
    fi
fi

# After a full check passed: the hashes of what it used, and when it began.
if [ "${1:-}" = "--record" ]; then
    if [ -f "$SWEEP/started" ] && ls "$SWEEP"/run/*.json >/dev/null 2>&1; then
        grep -ohE -- '-[0-9a-f]{16}' "$SWEEP"/run/*.json | cut -c2- | sort -u >"$SWEEP/live.new"
        # A list too short to be a whole check's (a message format cargo changed?): kept
        # the old one rather than lose everything.
        if [ "$(wc -l <"$SWEEP/live.new")" -ge 100 ]; then
            mv "$SWEEP/live.new" "$SWEEP/live"
            mv "$SWEEP/started" "$SWEEP/stamp"
        else
            echo "sweep: too few units recorded, the last list kept"
            rm -f "$SWEEP/live.new"
        fi
    fi
    exit 0
fi

size_gb() {
    # In KB, then GB (du -s is the same on macOS, Linux and Git Bash).
    local kb=0 dir k
    for dir in "${BUILDS[@]}"; do
        [ -d "$dir" ] || continue
        k=$(du -sk "$dir" 2>/dev/null | cut -f1)
        kb=$((kb + ${k:-0}))
    done
    echo $((kb / 1024 / 1024))
}

now=$(size_gb)
if [ "${1:-}" = "--now" ] || [ "$now" -ge "$LIMIT_GB" ]; then
    echo "target/ builds are ${now} GB (limit ${LIMIT_GB} GB): emptied, the next build starts afresh"
    rm -rf "${BUILDS[@]}" "$SWEEP"
    mkdir -p "$SWEEP"
    [ "${1:-}" = "--now" ] && exit 0
fi

# Entries (one per line) whose hash isn't in the list `live`.
dead() {
    awk -v live="$SWEEP/live" '
        BEGIN {
            while ((getline h < live) > 0) keep[h] = 1
            hex = "[0-9a-f]"; pat = "-"
            for (i = 0; i < 16; i++) pat = pat hex
        }
        {
            n = split($0, parts, "/"); name = parts[n]
            if (match(name, pat)) {
                h = substr(name, RSTART + 1, 16)
                rest = substr(name, RSTART + 17, 1)
                if ((rest == "" || rest == ".") && !(h in keep)) print
            }
        }'
}

# Object files in a folder (-R: and in its folders), each as "inode path" — read from the
# folders' listings (ls -f: unsorted, no file looked at one by one).
objects() {
    local dir="${!#}"
    ls -1fi "$@" 2>/dev/null | awk -v dir="$dir" '
        BEGIN { at = dir }
        /^[^ 0-9].*:$/ { at = substr($0, 1, length($0) - 1); next }
        {
            sub(/^ +/, "")
            i = index($0, " "); name = substr($0, i + 1)
            if (i > 0 && name ~ /\.o$/) print substr($0, 1, i - 1) " " at "/" name
        }'
}

# The lines ("inode path") of the units in the list `live`.
live_units() {
    awk -v live="$SWEEP/live" '
        BEGIN {
            while ((getline h < live) > 0) keep[h] = 1
            hex = "[0-9a-f]"; pat = "-"
            for (i = 0; i < 16; i++) pat = pat hex
        }
        {
            n = split($0, parts, "/"); name = parts[n]
            if (match(name, pat) && (substr(name, RSTART + 1, 16) in keep)) print
        }'
}

# What goes is gathered in one list (one path a line), removed at the end in one go.
GONE="$SWEEP/gone"
: >"$GONE"

case "$(uname)" in MINGW* | MSYS* | CYGWIN*) windows=1 ;; *) windows=0 ;; esac

for profile in "${PROFILES[@]}"; do
    [ -d "$profile" ] || continue
    if [ -f "$SWEEP/live" ] && [ -f "$SWEEP/stamp" ]; then
        for sub in deps .fingerprint build; do
            [ -d "$profile/$sub" ] || continue
            find "$profile/$sub" -mindepth 1 -maxdepth 1 ! -newer "$SWEEP/stamp" | dead \
                | while IFS= read -r entry; do
                    # A folder: kept if anything in it is newer than the stamp.
                    if [ -d "$entry" ] && [ -n "$(find "$entry" -newer "$SWEEP/stamp" | head -n 1)" ]; then
                        continue
                    fi
                    echo "$entry"
                done >>"$GONE"
        done
    fi
    inc="$profile/incremental"
    [ -d "$inc" ] || continue
    # Built without incremental caches (check.sh): none of them is read again.
    for none in "${NO_INCREMENTAL[@]}"; do
        if [ "$profile" = "$none" ]; then
            echo "$inc" >>"$GONE"
            continue 2
        fi
    done
    # The object files of the units (deps/) and of the caches (incremental/), "inode path"
    # (macOS: a unit's are links to its cache's; elsewhere there are none in deps/).
    objects "$profile/deps" >"$SWEEP/deps.o"
    objects -R "$inc" >"$SWEEP/incremental.o"
    : >"$SWEEP/incremental.gone"
    if [ -f "$SWEEP/live" ] && [ -f "$SWEEP/stamp" ]; then
        # A crate's cache untouched since the last full check began and for a while, and
        # whose objects no unit of that check links: gone.
        find "$inc" -mindepth 2 \( -newer "$SWEEP/stamp" -o -mmin "-$KEEP_INCREMENTAL_MIN" \) \
            | awk -v inc="$inc/" '{ rest = substr($0, length(inc) + 1); sub(/\/.*/, "", rest); print inc rest }' \
            | sort -u >"$SWEEP/incremental.recent"
        live_units <"$SWEEP/deps.o" | awk '{ print $1 }' | sort -u >"$SWEEP/used"
        find "$inc" -mindepth 1 -maxdepth 1 -type d | sort | comm -23 - "$SWEEP/incremental.recent" \
            | awk -v used="$SWEEP/used" -v objects="$SWEEP/incremental.o" -v inc="$inc/" '
                BEGIN { while ((getline i < used) > 0) linked[i] = 1 }
                { old[$0] = 1 }
                END {
                    while ((getline line < objects) > 0) {
                        split(line, f, " "); path = substr(line, length(f[1]) + 2)
                        rest = substr(path, length(inc) + 1); sub(/\/.*/, "", rest)
                        if (f[1] in linked) old[inc rest] = 0
                    }
                    for (dir in old) if (old[dir]) print dir
                }' >"$SWEEP/incremental.gone"
        cat "$SWEEP/incremental.gone" >>"$GONE"
    fi
    # A unit's object files kept beside it (macOS: its debug information points there), a
    # new name for each at every build: those no longer in a cache kept (an older build's,
    # its code changed since) go. Not on Windows (no such files; its inode numbers aren't to
    # be relied on), nor when builds run without incremental caches.
    if [ "$windows" = 0 ] && [ "${CARGO_INCREMENTAL:-1}" != 0 ] && [ -s "$SWEEP/incremental.o" ]; then
        awk -v gone="$SWEEP/incremental.gone" -v objects="$SWEEP/incremental.o" -v inc="$inc/" '
            BEGIN {
                while ((getline d < gone) > 0) dropped[d] = 1
                while ((getline line < objects) > 0) {
                    split(line, f, " "); path = substr(line, length(f[1]) + 2)
                    rest = substr(path, length(inc) + 1); sub(/\/.*/, "", rest)
                    if (!((inc rest) in dropped)) cached[f[1]] = 1
                }
            }
            !($1 in cached) { print substr($0, length($1) + 2) }' "$SWEEP/deps.o" >>"$GONE"
    fi
done
rm -f "$SWEEP/deps.o" "$SWEEP/incremental.o" "$SWEEP/incremental.recent" \
    "$SWEEP/incremental.gone" "$SWEEP/used"

removed=$(wc -l <"$GONE" | tr -d ' ')
if [ -n "${EDITOR_SWEEP_DRY:-}" ]; then
    sed "s|^$TARGET/|would remove |" "$GONE"
    rm -f "$GONE"
    exit 0
elif [ "$removed" -gt 0 ]; then
    tr '\n' '\0' <"$GONE" | xargs -0 rm -rf --
fi
rm -f "$GONE"

[ "$removed" -gt 0 ] && echo "sweep: ${removed} unused build entries removed"

# This check begins: what it uses is saved under run/ (check.sh), recorded if it passes.
rm -rf "$SWEEP/run"
mkdir -p "$SWEEP/run"
touch "$SWEEP/started"
exit 0
