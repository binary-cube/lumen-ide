#!/bin/sh
# Seeds a test server (any version from 9.2): drops what an earlier seed made, then makes
# the roles, the databases and their data again. Run as the image's superuser, on the
# server's socket (at first start by initdb.sh; later by `scripts/test-databases.sh seed`).
#
#   roles      editor (the tests' user: CREATEDB, not a superuser), editor_owner (owns
#              tables and a database; no login), editor_reader (read only)
#   databases  postgres (empty: the default one, as a user's source often names it),
#              shop (public, sales, audit), analytics (owned by editor_owner),
#              editor_private (editor may not connect)
set -eu

PASSWORD="${EDITOR_SEED_PASSWORD:-editor-test}"
DIR="$(dirname "$0")"
PSQL="psql -v ON_ERROR_STOP=1 -q -U postgres"

version=$($PSQL -d postgres -tAc 'SHOW server_version_num')
echo "seeding PostgreSQL $version"

$PSQL -d postgres <<SQL
SELECT pg_terminate_backend(pid) FROM pg_stat_activity
 WHERE datname IN ('shop', 'analytics', 'editor_private') AND pid <> pg_backend_pid();
DROP DATABASE IF EXISTS shop;
DROP DATABASE IF EXISTS analytics;
DROP DATABASE IF EXISTS editor_private;
DO \$\$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'editor') THEN
    EXECUTE 'DROP OWNED BY editor CASCADE';
    EXECUTE 'DROP ROLE editor';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'editor_reader') THEN
    EXECUTE 'DROP OWNED BY editor_reader CASCADE';
    EXECUTE 'DROP ROLE editor_reader';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'editor_owner') THEN
    EXECUTE 'DROP OWNED BY editor_owner CASCADE';
    EXECUTE 'DROP ROLE editor_owner';
  END IF;
END \$\$;
CREATE ROLE editor LOGIN CREATEDB PASSWORD '$PASSWORD';
CREATE ROLE editor_owner NOLOGIN;
CREATE ROLE editor_reader LOGIN PASSWORD '$PASSWORD';
ALTER ROLE editor_reader SET default_transaction_read_only = on;
CREATE DATABASE shop OWNER editor;
CREATE DATABASE analytics OWNER editor_owner;
CREATE DATABASE editor_private;
REVOKE CONNECT ON DATABASE editor_private FROM PUBLIC;
SQL

$PSQL -d shop -f "$DIR/shop.sql"
# What newer servers have: jsonb (9.4), materialized views (9.3), identity and partitions
# (10), generated columns (12).
if [ "$version" -ge 90300 ]; then $PSQL -d shop -f "$DIR/shop-93.sql"; fi
if [ "$version" -ge 90400 ]; then $PSQL -d shop -f "$DIR/shop-94.sql"; fi
if [ "$version" -ge 100000 ]; then $PSQL -d shop -f "$DIR/shop-10.sql"; fi
if [ "$version" -ge 120000 ]; then $PSQL -d shop -f "$DIR/shop-12.sql"; fi
$PSQL -d analytics -f "$DIR/analytics.sql"
$PSQL -d editor_private -c "CREATE TABLE hidden (id integer PRIMARY KEY, note text);
  INSERT INTO hidden VALUES (1, 'not for editor');"
# The marker the health check waits for: written last.
$PSQL -d shop -c "ANALYZE; CREATE TABLE public.seed_info AS SELECT $version AS version, now() AS seeded_at;
  ALTER TABLE public.seed_info OWNER TO editor;"
echo "seeded PostgreSQL $version"
