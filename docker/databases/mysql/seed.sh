#!/bin/sh
# Seeds a MySQL (5.7+) or MariaDB (10.11+) test server: drops what an earlier seed made,
# then makes the users, the databases and their data again. Run as root, on the server's
# socket (at first start by initdb.sh; later by `scripts/test-databases.sh seed`).
#
#   users      editor (the tests' user: shop and analytics), editor_reader (SELECT only)
#   databases  shop, analytics, editor_private (editor has no rights there)
#
# MySQL has no owners: "a table owned by another role" is a view with another DEFINER
# (SQL SECURITY DEFINER), which editor may read without the table under it.
set -eu

PASSWORD="${EDITOR_SEED_PASSWORD:-editor-test}"
ROOT="${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-editor-test-root}}"
DIR="$(dirname "$0")"
CLIENT=mysql
command -v mysql >/dev/null 2>&1 || CLIENT=mariadb
SQL="$CLIENT --protocol=socket -uroot -p$ROOT"

version=$($SQL -N -e 'SELECT VERSION()')
echo "seeding $version"
case "$version" in
    *MariaDB*) flavor=mariadb ;;
    5.7*) flavor=mysql57 ;;
    *) flavor=mysql8 ;;
esac

$SQL <<SQL
DROP DATABASE IF EXISTS shop;
DROP DATABASE IF EXISTS analytics;
DROP DATABASE IF EXISTS editor_private;
DROP USER IF EXISTS 'editor'@'%';
DROP USER IF EXISTS 'editor_reader'@'%';
DROP USER IF EXISTS 'editor_owner'@'%';
CREATE DATABASE shop CHARACTER SET utf8mb4;
CREATE DATABASE analytics CHARACTER SET utf8mb4;
CREATE DATABASE editor_private CHARACTER SET utf8mb4;
CREATE USER 'editor'@'%' IDENTIFIED BY '$PASSWORD';
CREATE USER 'editor_reader'@'%' IDENTIFIED BY '$PASSWORD';
CREATE USER 'editor_owner'@'%' IDENTIFIED BY '$PASSWORD' ACCOUNT LOCK;
GRANT ALL PRIVILEGES ON shop.* TO 'editor'@'%';
GRANT SELECT, SHOW VIEW ON analytics.* TO 'editor'@'%';
GRANT SELECT, SHOW VIEW ON shop.* TO 'editor_reader'@'%';
GRANT SELECT, SHOW VIEW ON analytics.* TO 'editor_reader'@'%';
GRANT ALL PRIVILEGES ON shop.* TO 'editor_owner'@'%';
GRANT SELECT ON editor_private.* TO 'editor_owner'@'%';
FLUSH PRIVILEGES;
SQL

$SQL editor_private -e "CREATE TABLE hidden (id int PRIMARY KEY, note text);
  INSERT INTO hidden VALUES (1, 'not for editor');"
$SQL shop < "$DIR/shop.sql"
if [ "$flavor" = mysql8 ]; then $SQL shop < "$DIR/shop-80.sql"; fi
$SQL analytics < "$DIR/analytics.sql"
# The marker the health check waits for: written last.
$SQL shop -e "CREATE TABLE seed_info AS SELECT '$version' AS version, NOW() AS seeded_at;"
echo "seeded $version"
