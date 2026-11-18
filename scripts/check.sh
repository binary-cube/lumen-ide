#!/usr/bin/env bash
# The checks before a commit, run in parallel (each in its own job; the clippy ones in their
# own target dirs, so nothing waits on another's lock — only metadata: small):
#
#   ./scripts/check.sh            fmt, then clippy (host), clippy (Windows), tests, check-deps
#   ./scripts/check.sh -p a -p b  the same, the tests on those packages only
#
# First, what the last full check didn't use goes from target/ (scripts/sweep.sh); each job
# saves which units it used (cargo's JSON messages, under target/.sweep/run), recorded
# once a full check passed. The clippy jobs build without incremental caches (each change
# is checked once there: they'd only take room).
#
# Clippy for Windows looks only at the crates with code or dependencies of their own for a
# platform (`cfg(windows)`, `cfg(unix)`, `[target.…]`), found anew each run: the rest is the
# same code on every system, already checked by the host's clippy.
#
# Each job's output goes to target/checks/<job>.log; the end shows which passed.
# Windows clippy needs the target x86_64-pc-windows-gnu and MinGW-w64 (see build.sh).

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOGS="$ROOT/target/checks"
mkdir -p "$LOGS"
cd "$ROOT"

# `-p a -p b`: what the tests look at (default: everything).
# The whole workspace's tests include the snapshots' (the app's `snapshot` feature).
SCOPE=("--workspace" "--features" "app/snapshot")
if [ "$#" -gt 0 ]; then
    SCOPE=("$@")
fi

# target/ kept small (see sweep.sh); this check begun.
"$ROOT/scripts/sweep.sh"
RUN="$ROOT/target/.sweep/run"
FULL=1
[ "$#" -gt 0 ] && FULL=0

cargo fmt --all || { echo "fmt failed"; exit 1; }

# The crates with platform-specific code or dependencies.
WINDOWS=()
# Their tests too: a test using a function of one system only fails to build on the other.
for dir in $( (grep -rlE 'cfg!?\((windows|unix|not\((windows|unix)\)|target_os)' crates --include='*.rs' \
        | sed -E 's#(crates/([^/]+/)*[^/]+)/(src|tests)/.*#\1#';
    grep -l '^\[target\.' crates/*/Cargo.toml crates/*/*/Cargo.toml crates/*/*/*/Cargo.toml \
        | xargs -n1 dirname) | sort -u); do
    name=$(grep -m1 '^name' "$dir/Cargo.toml" | sed -E 's/name = "(.*)"/\1/')
    WINDOWS+=("-p" "$name")
done

run() {
    local name="$1"
    shift
    if "$@" >"$LOGS/$name.log" 2>&1; then
        echo "ok      $name"
    else
        echo "FAILED  $name  (target/checks/$name.log)"
        return 1
    fi
}

clippy_clean() {
    # Clippy's warnings count as failures; a dependency's build script output (a C
    # compiler's notes: `warning: crate@1.2.3: …`) doesn't.
    local out="$1"
    shift
    "$@" 2>&1 | tee "$out.raw" | grep -E "^(warning|error)" \
        | grep -vE "^warning: [A-Za-z0-9_-]+@[0-9][^ ]*: " && return 1
    return 0
}

# Cargo's messages, rendered as usual for the log; its JSON lines (the units built or
# found fresh) to `$1`, for sweep.sh.
units_to() {
    awk -v out="$1" '/^\{"reason":/ { print > out; next } { print; fflush() }'
}

# The app with its snapshots too (the `snapshot` feature, only in the builds that render
# them): after the rest, in the same target dir (only the app is built again).
clippy_host() {
    export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$ROOT/target/check-host"
    cargo clippy --workspace --all-targets --message-format=json-render-diagnostics \
        >"$RUN/clippy-host.json" \
        && cargo clippy -p app --all-targets --features snapshot \
            --message-format=json-render-diagnostics >>"$RUN/clippy-host.json"
}

clippy_windows() {
    CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$ROOT/target/check-windows" \
        cargo clippy "${WINDOWS[@]}" --all-targets --target x86_64-pc-windows-gnu \
        --message-format=json-render-diagnostics >"$RUN/clippy-windows.json"
}

tests() {
    cargo test -q --message-format=json-render-diagnostics "${SCOPE[@]}" \
        | units_to "$RUN/tests.json"
}

# `cargo xtask check-deps`, its build's units seen too.
check_deps() {
    cargo run --quiet --message-format=json-render-diagnostics --package xtask -- check-deps \
        | units_to "$RUN/check-deps.json"
}

run clippy-host clippy_clean "$LOGS/clippy-host" clippy_host &
run clippy-windows clippy_clean "$LOGS/clippy-windows" clippy_windows &
run tests tests &
run check-deps check_deps &

failed=0
for job in $(jobs -p); do
    wait "$job" || failed=1
done
# What a whole check used: what the next one keeps (see sweep.sh).
if [ "$failed" = 0 ] && [ "$FULL" = 1 ]; then
    "$ROOT/scripts/sweep.sh" --record
fi
exit "$failed"
