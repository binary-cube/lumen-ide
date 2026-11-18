# The local database test matrix

Throwaway PostgreSQL, MySQL and MariaDB servers, one per version, every one seeded with the
same data, for the drivers' integration tests. Nothing here is a real server: the passwords
are the ones below, the data is made up, and each server's data lives in its container (gone
with `down`).

```sh
scripts/test-databases.sh up            # start them all, wait until each is seeded
eval "$(scripts/test-databases.sh urls)" # EDITOR_TEST_POSTGRES_URLS / EDITOR_TEST_MYSQL_URLS
cargo test -p db-postgres --test matrix  # the driver against every running PostgreSQL
cargo test -p database-explorer --test it postgres  # the "Database" window on each of them
cargo test -p db-mysql --test matrix     # the driver against every running MySQL / MariaDB
cargo test -p database-explorer --test it mysql     # the "Database" window on each of them
scripts/test-databases.sh seed pg14     # seed one again, from scratch
scripts/test-databases.sh down          # stop and remove them
```

Only the compose project `editor-test-db` is touched: containers `editor-test-*`, ports on
127.0.0.1 only.

| Service       | Image               | Port  | Note                                   |
|---------------|---------------------|-------|----------------------------------------|
| `pg92`        | postgres:9.2        | 55400 | amd64 only: emulated on arm64          |
| `pg94`        | postgres:9.4-alpine | 55401 |                                        |
| `pg96`        | postgres:9.6-alpine | 55402 |                                        |
| `pg12`        | postgres:12-alpine  | 55403 |                                        |
| `pg14`        | postgres:14-alpine  | 55404 |                                        |
| `pg16`        | postgres:16-alpine  | 55405 |                                        |
| `pg17`        | postgres:17-alpine  | 55406 |                                        |
| `pg18`        | postgres:18-alpine  | 55407 |                                        |
| `mysql57`     | mysql:5.7           | 55410 | amd64 only: emulated on arm64          |
| `mysql80`     | mysql:8.0           | 55411 |                                        |
| `mysql84`     | mysql:8.4           | 55412 |                                        |
| `mariadb1011` | mariadb:10.11       | 55413 |                                        |
| `mariadb114`  | mariadb:11.4        | 55414 |                                        |
| `mariadb118`  | mariadb:11.8        | 55415 |                                        |

## Users (every server)

| User            | Password           | What it may do                                     |
|-----------------|--------------------|----------------------------------------------------|
| `editor`        | `editor-test`      | the tests' user: owns `shop`, reads `analytics`    |
| `editor_reader` | `editor-test`      | reads `shop` and `analytics`, writes nothing       |
| `editor_owner`  | — (no login)       | owns tables (PostgreSQL), a view's definer (MySQL) |
| `postgres` / `root` | `editor-test-root` | the superuser, used only by the seed           |

## The data

- **Databases**: `postgres` (PostgreSQL: empty — the default database a source often names,
  while the data lives in the others), `shop`, `analytics` (owned by `editor_owner`),
  `editor_private` (`editor` may not connect / read).
- **PostgreSQL `shop`**: schemas `public`, `sales`, `audit` (owned by `editor_owner`);
  primary, foreign (with a composite one), unique and check keys; expression and partial
  indexes; a view, a materialized view (9.3+), an enum, `json` / `jsonb` (9.4+) with a GIN
  index, an identity column and a partitioned table (10+), a stored generated column (12+),
  comments; ~16k rows; tables owned by `editor_owner`, one readable by `editor`
  (`audit.events`, `public.legacy_notes`), one not (`audit.secrets`).
- **MySQL / MariaDB `shop`**: the same tables (no schemas: databases), `enum` / `set`,
  `json`, a stored generated column, check constraints (enforced from MySQL 8.0.16 /
  MariaDB), a functional and a descending index (MySQL 8.0+), a view whose definer is
  `editor_owner` over a table `editor` may not read.

The seed scripts (`postgres/seed.sh`, `mysql/seed.sh`) run at a container's first start and
again with `scripts/test-databases.sh seed`; each drops what it made before.
