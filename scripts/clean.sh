#!/usr/bin/env bash
# Frees the disk target/ takes (builds are made again when needed; nothing else is touched).
#
#   ./scripts/clean.sh            what the last full check didn't use (scripts/sweep.sh):
#                                 the next build stays quick
#   ./scripts/clean.sh --builds   the checks' and tests' builds emptied (target/debug,
#                                 check-host, check-windows): the next check builds afresh;
#                                 the release builds (build.sh's) kept
#   ./scripts/clean.sh --all      everything under target/ (`cargo clean`), the release and
#                                 Windows builds too
#   --dry                         what would go, nothing removed
#
# Its size is told before and after.

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"

MODE="sweep"
DRY=0
for arg in "$@"; do
    case "$arg" in
    --builds) MODE="builds" ;;
    --all) MODE="all" ;;
    --dry) DRY=1 ;;
    -h | --help)
        sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "clean: unknown option $arg (see --help)" >&2
        exit 2
        ;;
    esac
done

size() {
    if [ -d "$TARGET" ]; then du -sh "$TARGET" 2>/dev/null | cut -f1; else echo "0"; fi
}

echo "target/: $(size)"
case "$MODE" in
sweep)
    if [ "$DRY" = 1 ]; then
        EDITOR_SWEEP_DRY=1 "$ROOT/scripts/sweep.sh"
    else
        "$ROOT/scripts/sweep.sh"
    fi
    ;;
builds)
    if [ "$DRY" = 1 ]; then
        du -sh "$TARGET"/debug "$TARGET"/check-host "$TARGET"/check-windows 2>/dev/null || true
        echo "(would be removed)"
    else
        "$ROOT/scripts/sweep.sh" --now
    fi
    ;;
all)
    if [ "$DRY" = 1 ]; then
        echo "target/ would be removed whole"
    else
        (cd "$ROOT" && cargo clean)
    fi
    ;;
esac
echo "target/: $(size)"
