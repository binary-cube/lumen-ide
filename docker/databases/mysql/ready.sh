#!/bin/sh
# The health check: over TCP (the server the image starts while seeding has no network),
# and only once the seed wrote its last table.
CLIENT=mysql
command -v mysql >/dev/null 2>&1 || CLIENT=mariadb
ROOT="${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-editor-test-root}}"
$CLIENT -h 127.0.0.1 -P 3306 -uroot "-p$ROOT" -N -e 'SELECT 1 FROM shop.seed_info' 2>/dev/null | grep -q 1
