#!/usr/bin/env bash
# The local database test matrix (docker/databases/compose.yaml): our own throwaway
# PostgreSQL, MySQL and MariaDB servers, every one seeded with the same data.
#
#   scripts/test-databases.sh up [service…]     start them (all, or those named), wait until
#                                               each is seeded and healthy
#   scripts/test-databases.sh down              stop and remove them (their data with them)
#   scripts/test-databases.sh seed [service…]   seed the running ones again (from scratch)
#   scripts/test-databases.sh urls              the exports the integration tests read:
#                                               eval "$(scripts/test-databases.sh urls)"
#   scripts/test-databases.sh status            what runs, and its health
#
# Only the compose project `editor-test-db` is ever touched (containers `editor-test-*`,
# ports 127.0.0.1:55400–55419): nothing else running on the machine.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE=(docker compose -f "$ROOT/docker/databases/compose.yaml" -p editor-test-db)
PASSWORD="editor-test"

# service → host port (as compose.yaml maps them).
POSTGRES=(pg92:55400 pg94:55401 pg96:55402 pg12:55403 pg14:55404 pg16:55405 pg17:55406 pg18:55407)
MYSQL=(mysql57:55410 mysql80:55411 mysql84:55412 mariadb1011:55413 mariadb114:55414 mariadb118:55415)

running() {
    "${COMPOSE[@]}" ps --status running --services 2>/dev/null
}

urls() {
    local up pg=() my=()
    up="$(running)"
    for entry in "${POSTGRES[@]}"; do
        if grep -qx "${entry%%:*}" <<<"$up"; then
            pg+=("postgresql://editor:$PASSWORD@127.0.0.1:${entry##*:}/postgres?sslmode=disable")
        fi
    done
    for entry in "${MYSQL[@]}"; do
        if grep -qx "${entry%%:*}" <<<"$up"; then
            my+=("mysql://editor:$PASSWORD@127.0.0.1:${entry##*:}/shop")
        fi
    done
    local IFS=,
    echo "export EDITOR_TEST_POSTGRES_URLS='${pg[*]-}'"
    echo "export EDITOR_TEST_MYSQL_URLS='${my[*]-}'"
}

seed() {
    local services=("$@") service
    if [ "${#services[@]}" -eq 0 ]; then
        # (bash 3.2, macOS's: no mapfile.)
        while IFS= read -r service; do services+=("$service"); done < <(running)
    fi
    for service in "${services[@]+"${services[@]}"}"; do
        echo "seeding $service"
        "${COMPOSE[@]}" exec -T "$service" sh /seed/seed.sh
    done
}

case "${1:-}" in
    up)
        shift
        "${COMPOSE[@]}" up -d --wait --wait-timeout 600 "$@"
        "${COMPOSE[@]}" ps
        ;;
    down)
        "${COMPOSE[@]}" down --volumes --remove-orphans
        ;;
    seed)
        shift
        seed "$@"
        ;;
    urls)
        urls
        ;;
    status)
        "${COMPOSE[@]}" ps --all
        ;;
    *)
        sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
        exit 2
        ;;
esac
