#!/usr/bin/env bash
# Release builds, packaged into dist/. The app's name comes from brand.toml (`name`, `id`,
# `bundle_id`); below, <Name> / <id> stand for them.
#
#   ./scripts/build.sh                    interactive menu
#   ./scripts/build.sh mac [--universal]  <Name>.app → dist/<id>-<ver>-macos-<arch>.zip
#   ./scripts/build.sh windows            <id>.exe   → dist/<id>-<ver>-windows-x64.zip
#   ./scripts/build.sh linux              <id>       → dist/<id>-<ver>-linux-<arch>.tar.gz
#   ./scripts/build.sh all                everything buildable on the current system
#   --with-servers (or EDITOR_WITH_SERVERS=1)  a full, offline build: the language servers inside
#   --full       the build handed out (the `release` profile: fat LTO, one codegen unit —
#                its last step on one core, however many there are): the release workflows'
#   (default)    a local build (the `dist-fast` profile: thin LTO, the code optimized on
#                every core): several times quicker, a little bigger, about as fast to use
#   --fast       the default (kept for the habit)
#
# By default the builds carry no language servers (plan 43i): the app downloads each one
# the first time a file needs it, once for every project, from the sources pinned in its
# manifest (`cargo xtask lsp-manifest`, built into the app). With --with-servers a build
# carries them all (plan 43: Node and the servers, `packaging/lsp.toml`), made for its
# platform by `cargo xtask lsp-bundle` (downloaded once into target/lsp-cache, checked) —
# `lsp/` beside the app: `<Name>.app/Contents/Resources/lsp`, `lsp\` beside the `.exe`,
# `lsp/` beside the Linux binary; the app uses those first.
#
# Windows from macOS / Linux: needs MinGW-w64 (macOS: `brew install mingw-w64`) and the
# Rust target x86_64-pc-windows-gnu (the script adds it). On Windows: scripts/build.ps1.
# --universal (macOS): a single binary for Apple Silicon and Intel.
#
# Built without the app's `snapshot` feature: the `--snapshot` screens (scripts/ui-snapshots.sh)
# stay out of what's handed out.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"
HOST="$(uname -s)"
UNIVERSAL=0
# The Cargo profile the app is built with: `dist-fast` here, `release` with --full (what's
# handed out: the release workflows pass it).
PROFILE="dist-fast"
WITH_SERVERS="${EDITOR_WITH_SERVERS:-0}"
# The product's name, from brand.toml.
brand() { grep -m1 "^$1 *=" "$ROOT/brand.toml" | sed -E 's/.*"(.*)".*/\1/'; }
NAME="$(brand name)"
ID="$(brand id)"
BUNDLE_ID="$(brand bundle_id)"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '  \033[36m→\033[0m %s\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || fail "\`$1\` is missing${2:+ ($2)}"; }

# Work run beside the app's build: Cargo holds a build folder while it builds, so what runs
# at the same time builds in a folder of its own (made once, kept after).
TOOLS_TARGET="$ROOT/target/tools"
CROSS_TARGET="$ROOT/target/cross"
LOGS="$ROOT/target/build-logs"

# `$1` (a name) run in the background, its output in target/build-logs/<name>.log; its pid
# in the variable named `$2`.
beside() {
  local name="$1" var="$2"
  shift 2
  mkdir -p "$LOGS"
  "$@" >"$LOGS/$name.log" 2>&1 &
  printf -v "$var" '%s' "$!"
}

# The background work `$2` (its pid) waited for: on failure its log's end shown, and out.
await() {
  local name="$1" pid="$2"
  if ! wait "$pid"; then
    tail -n 30 "$LOGS/$name.log" >&2
    fail "$name failed (the whole log: target/build-logs/$name.log)"
  fi
}

# What's built besides the app: our own language server, in a full build.
extra_packages() {
  [[ $WITH_SERVERS == 1 ]] && echo "-p app -p language-server"
}

# The language servers for platform $1, made into folder $2 (see packaging/lsp.toml).
lsp_bundle() {
  (cd "$ROOT" && CARGO_TARGET_DIR="$TOOLS_TARGET" cargo xtask lsp-bundle --platform "$1" --out "$2") || fail "the language servers couldn't be bundled"
  ok "lsp/ for $1: $(du -sh "$2" | cut -f1)"
}

ensure_target() {
  if ! rustup target list --installed | grep -qx "$1"; then
    info "adding the Rust target $1"
    rustup target add "$1"
  fi
}

build_mac() {
  [[ "$HOST" == "Darwin" ]] || fail "the macOS build runs only on macOS"
  bold "macOS"
  local arch binary
  # Beside the build: the app icon (rendered again only when its master or its code
  # changed) and, in a full build, the language servers (downloaded, checked).
  local iconset="$ROOT/target/icon/$NAME.iconset" icon_pid="" servers_pid=""
  if [[ -d "$iconset" && "$iconset" -nt "$ROOT/assets/app-icon.png" \
        && "$iconset" -nt "$ROOT/xtask/src/app_icon.rs" ]]; then
    info "the app icon (kept: unchanged)"
  else
    info "the app icon (beside the build)"
    beside icon icon_pid bash -c "cd '$ROOT' && CARGO_TARGET_DIR='$TOOLS_TARGET' cargo run --quiet -p xtask -- icon >/dev/null && touch '$iconset'"
  fi
  local servers=""
  if [[ $WITH_SERVERS == 1 ]]; then
    servers="$(mktemp -d)"
    info "the language servers (beside the build)"
    if [[ $UNIVERSAL == 1 ]]; then
      beside servers servers_pid bash -c "'$0' __lsp macos-arm64 '$servers/lsp' && '$0' __lsp macos-x64 '$servers/intel'"
    elif [[ "$(uname -m)" == arm64 ]]; then
      beside servers servers_pid "$0" __lsp macos-arm64 "$servers/lsp"
    else
      beside servers servers_pid "$0" __lsp macos-x64 "$servers/lsp"
    fi
  fi
  if [[ $UNIVERSAL == 1 ]]; then
    ensure_target aarch64-apple-darwin
    ensure_target x86_64-apple-darwin
    info "building for Apple Silicon and Intel"
    (cd "$ROOT" && cargo build --profile "$PROFILE" $(extra_packages) --target aarch64-apple-darwin --target x86_64-apple-darwin)
    binary="$ROOT/target/$ID-universal"
    lipo -create -output "$binary" \
      "$ROOT/target/aarch64-apple-darwin/$PROFILE/$ID" \
      "$ROOT/target/x86_64-apple-darwin/$PROFILE/$ID"
    arch="universal"
  else
    info "building (the $PROFILE profile)"
    (cd "$ROOT" && cargo build --profile "$PROFILE" $(extra_packages))
    binary="$ROOT/target/$PROFILE/$ID"
    arch="$(uname -m)"
  fi

  [[ -n $icon_pid ]] && await icon "$icon_pid"
  local app="$DIST/$NAME.app"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$binary" "$app/Contents/MacOS/$ID"
  if [[ $WITH_SERVERS == 1 && $UNIVERSAL == 1 ]]; then
    lipo -create -output "$app/Contents/MacOS/language-server" \
      "$ROOT/target/aarch64-apple-darwin/$PROFILE/language-server" \
      "$ROOT/target/x86_64-apple-darwin/$PROFILE/language-server"
  elif [[ $WITH_SERVERS == 1 ]]; then
    cp "$ROOT/target/$PROFILE/language-server" "$app/Contents/MacOS/language-server"
  fi
  # macOS takes only numbers there (1.0.0 for 1.0.0-beta.1); the full version is in the zip's name.
  local plist_version="${VERSION%%-*}"
  sed -e "s/__VERSION__/$plist_version/g" -e "s/__NAME__/$NAME/g" -e "s/__ID__/$ID/g" \
    -e "s/__BUNDLE_ID__/$BUNDLE_ID/g" "$ROOT/packaging/macos/Info.plist" >"$app/Contents/Info.plist"
  iconutil -c icns "$ROOT/target/icon/$NAME.iconset" -o "$app/Contents/Resources/$NAME.icns"
  if [[ $WITH_SERVERS != 1 ]]; then
    info "no language servers inside (downloaded when first needed; --with-servers for a full build)"
  elif [[ $UNIVERSAL == 1 ]]; then
    await servers "$servers_pid"
    mv "$servers/lsp" "$app/Contents/Resources/lsp"
    ok "the language servers: $(du -sh "$app/Contents/Resources/lsp" | cut -f1)"
    # One program for both, each one's two official builds joined: Node and the native
    # servers (clangd, lua-language-server, zls, taplo) — every Mach-O program of the arm64
    # bundle with an Intel twin.
    local intel="$servers/intel-root"
    mkdir -p "$intel" && mv "$servers/intel" "$intel/lsp"
    local lsp="$app/Contents/Resources/lsp" program twin
    while IFS= read -r program; do
      twin="$intel/lsp/${program#"$lsp/"}"
      if [[ -f $twin ]] && file -b "$program" | grep -q '^Mach-O' && ! cmp -s "$program" "$twin"; then
        lipo -create -output "$program.universal" "$program" "$twin"
        chmod +x "$program.universal"
        mv "$program.universal" "$program"
      fi
    done < <(find "$lsp" -type f -perm -u+x)
    rm -rf "$servers"
  else
    await servers "$servers_pid"
    mv "$servers/lsp" "$app/Contents/Resources/lsp"
    rm -rf "$servers"
    ok "the language servers: $(du -sh "$app/Contents/Resources/lsp" | cut -f1)"
  fi
  # Ad-hoc signature: without it, macOS refuses arm64 apps copied from another Mac.
  codesign --force --deep --sign - "$app" >/dev/null 2>&1 || info "ad-hoc signing failed (going on)"

  local zip="$DIST/$ID-$VERSION-macos-$arch.zip"
  rm -f "$zip"
  ditto -c -k --keepParent "$app" "$zip"
  ok "$(basename "$zip")  (in the zip: $NAME.app)"
}

build_windows() {
  bold "Windows x64"
  local target exe
  case "$HOST" in
    MINGW* | MSYS* | CYGWIN*)
      target="x86_64-pc-windows-msvc"
      ;;
    *)
      need x86_64-w64-mingw32-gcc "macOS: brew install mingw-w64; Linux: the mingw-w64 package"
      target="x86_64-pc-windows-gnu"
      ;;
  esac
  ensure_target "$target"
  # Its build folder: target/, or its own when it's built beside another (`all`).
  local dir="${WINDOWS_TARGET:-$ROOT/target}"
  info "building ($target)"
  (cd "$ROOT" && CARGO_TARGET_DIR="$dir" cargo build --profile "$PROFILE" $(extra_packages) --target "$target")
  exe="$dir/$target/$PROFILE/$ID.exe"

  local zip="$DIST/$ID-$VERSION-windows-x64.zip"
  local stage
  stage="$(mktemp -d)"
  cp "$exe" "$stage/$ID.exe"
  rm -f "$zip"
  if [[ $WITH_SERVERS == 1 ]]; then
    info "the language servers"
    lsp_bundle windows-x64 "$stage/lsp"
    cp "$dir/$target/$PROFILE/language-server.exe" "$stage/"
    (cd "$stage" && zip -qr "$zip" "$ID.exe" language-server.exe lsp)
  else
    (cd "$stage" && zip -qr "$zip" "$ID.exe")
  fi
  rm -rf "$stage"
  ok "$(basename "$zip")"
}

build_linux() {
  [[ "$HOST" == "Linux" ]] || fail "the Linux build runs on Linux"
  bold "Linux"
  info "building (the $PROFILE profile)"
  (cd "$ROOT" && cargo build --profile "$PROFILE" $(extra_packages))
  local tar="$DIST/$ID-$VERSION-linux-$(uname -m).tar.gz"
  if [[ $WITH_SERVERS == 1 ]]; then
    info "the language servers"
    lsp_bundle linux-x64 "$ROOT/target/$PROFILE/lsp"
    tar -czf "$tar" -C "$ROOT/target/$PROFILE" "$ID" language-server lsp
  else
    tar -czf "$tar" -C "$ROOT/target/$PROFILE" "$ID"
  fi
  ok "$(basename "$tar")"
}

# Everything buildable here, the Windows build beside the system's own: with fat LTO each
# one's last step runs on one core, so the two take little more than the longer one. The
# Windows build in a folder of its own (Cargo holds one while it builds), its output in
# target/build-logs/windows.log. Twice the memory meanwhile.
build_all() {
  local windows_pid
  case "$HOST" in
    Darwin | Linux)
      info "Windows x64 beside (its output: target/build-logs/windows.log)"
      beside windows windows_pid env WINDOWS_TARGET="$CROSS_TARGET" "$0" windows $(flags)
      if [[ "$HOST" == Darwin ]]; then build_mac; else build_linux; fi
      bold "Windows x64"
      info "waiting for it"
      await windows "$windows_pid"
      grep '✓' "$LOGS/windows.log" | tail -n 1 || true
      ;;
    *) build_windows ;;
  esac
}

# The options given, for a build started again by this script (`all`'s Windows one).
flags() {
  [[ $PROFILE == release ]] && printf -- '--full '
  [[ $WITH_SERVERS == 1 ]] && printf -- '--with-servers '
  return 0
}

interactive() {
  bold "$NAME $VERSION — build"
  local options=()
  [[ "$HOST" == "Darwin" ]] && options+=("macOS ($(uname -m))" "macOS universal (Apple Silicon + Intel)")
  [[ "$HOST" == "Linux" ]] && options+=("Linux")
  options+=("Windows x64" "All" "Quit")
  PS3="Choose: "
  select choice in "${options[@]}"; do
    case "$choice" in
      "macOS ("*) build_mac; break ;;
      "macOS universal"*) UNIVERSAL=1; build_mac; break ;;
      "Linux") build_linux; break ;;
      "Windows x64") build_windows; break ;;
      "All") build_all; break ;;
      "Quit") exit 0 ;;
      *) echo "not an option" ;;
    esac
  done
}

need cargo "install Rust: https://rustup.rs"
mkdir -p "$DIST"

COMMAND=""
# Internal: `__lsp <platform> <folder>` (the language servers made beside a build).
if [[ "${1:-}" == __lsp ]]; then
  LSP_ARGS=("$2" "$3")
  set -- __lsp
fi
for arg in "$@"; do
  case "$arg" in
    --universal) UNIVERSAL=1 ;;
    --fast) PROFILE="dist-fast" ;;
    --full) PROFILE="release" ;;
    --with-servers) WITH_SERVERS=1 ;;
    mac | macos) COMMAND="mac" ;;
    windows | win) COMMAND="windows" ;;
    linux) COMMAND="linux" ;;
    all) COMMAND="all" ;;
    stubs) COMMAND="stubs" ;;
    # Internal: the language servers for a platform into a folder (run beside a build).
    __lsp) COMMAND="lsp" ;;
    -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
    *) fail "unknown argument: $arg (see --help)" ;;
  esac
done

case "$COMMAND" in
  mac) build_mac ;;
  windows) build_windows ;;
  linux) build_linux ;;
  all) build_all ;;
  lsp) lsp_bundle "${LSP_ARGS[0]}" "${LSP_ARGS[1]}"; exit 0 ;;
  # The language data: PHP stubs (new PHP releases found and added automatically) and the
  # CSS / HTML / JavaScript data (from MDN's public-domain data), SQLite's functions.
  stubs) (cd "$ROOT" && cargo xtask php-stubs --update && cargo xtask web-data && cargo xtask sql-data) ;;
  "") interactive ;;
esac
echo
ok "done — the files are in dist/"
