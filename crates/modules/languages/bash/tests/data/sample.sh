#!/usr/bin/env bash
# Backs up a folder, keeping the last few copies.
set -euo pipefail

readonly KEEP=5
target="${1:-$HOME/backups}"

backup() {
    local source="$1" stamp
    stamp=$(date +%Y%m%d)
    tar -czf "$target/$stamp.tar.gz" "$source" 2>/dev/null || return 1
    echo "saved $source ($stamp)"
    [ $? -eq 0 ]
}

if [[ ! -d "$target" ]]; then
    mkdir -p "$target"
elif [[ $target =~ ^/tmp ]]; then
    echo 'a temporary place' >&2
fi

for dir in src docs; do
    backup "$dir"
done

case "$(uname -s)" in
    Darwin) open "$target" ;;
    *) ls -1 "$target" | head -n $((KEEP + 1)) ;;
esac

cat <<NOTE
done: $?
NOTE
