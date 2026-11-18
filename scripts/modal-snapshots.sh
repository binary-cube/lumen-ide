#!/usr/bin/env bash
# The modals and popovers only: `ui-snapshots.sh`'s "modals" group (kept for the habit).
#
#   scripts/modal-snapshots.sh record|compare <dir>
exec "$(dirname "$0")/ui-snapshots.sh" "${1:-}" "${2:-}" modals
