//! The catalog queries, written for the server's version (pure: tested without a server).
//! Only what 9.2 already reads, plus a newer server's own columns where it has them
//! (identity 10+, partitions 10+, generated columns 12+, `INCLUDE` columns 11+); every
//! value cast to text, read back by [`crate::PgSession`]. No `LATERAL` (9.3+), no
//! `WITH ORDINALITY` (9.4+), no `array_position` (9.5+): set-returning functions in the
//! select list, which every version runs.

use database::postgres::{Feature, supports};

/// The schemas left out of a tree (the server's own).
const SYSTEM: &str = "n.nspname NOT IN ('pg_catalog', 'information_schema') \
     AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%'";

/// The relations listed as tables or views: tables, views, materialized views (9.3),
/// foreign tables; partitioned tables (10+) once, their partitions not. With what the rows
/// are estimated from, cheaply: the planner's `reltuples` / `relpages`, and the statistics
/// collector's live rows (for a table never analyzed).
pub fn tables(version: u32, one_schema: bool) -> String {
    let mut kinds = vec!["'r'", "'v'", "'f'"];
    if version >= 90300 {
        kinds.push("'m'");
    }
    let partitions = supports(version, Feature::Identity);
    if partitions {
        kinds.push("'p'");
    }
    let mut sql = format!(
        "SELECT n.nspname::text, c.relname::text, c.relkind::text, c.reltuples::text, \
         c.relpages::text, pg_catalog.pg_stat_get_live_tuples(c.oid)::text \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ({}) AND {SYSTEM}",
        kinds.join(", ")
    );
    if partitions {
        sql.push_str(" AND NOT c.relispartition");
    }
    if one_schema {
        sql.push_str(" AND n.nspname = $1");
    }
    sql.push_str(" ORDER BY 1, 2");
    sql
}

/// The relations a query may read from besides those [`tables`] lists: sequences, and
/// partitions (10+).
pub fn relations(version: u32) -> String {
    let partitions = supports(version, Feature::Identity);
    format!(
        "SELECT n.nspname::text, c.relname::text FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE (c.relkind = 'S'{}) AND {SYSTEM} ORDER BY 1, 2",
        if partitions { " OR c.relispartition" } else { "" }
    )
}

/// Every schema of the database but the server's own, whoever owns it (their names):
/// `pg_namespace`, never `information_schema.schemata`, which before 10 lists only the
/// schemas the user owns (and after, only those it has a privilege on).
pub fn schemas() -> String {
    format!("SELECT n.nspname::text FROM pg_catalog.pg_namespace n WHERE {SYSTEM} ORDER BY 1")
}

/// A table's columns (`$1`: its quoted name): name, type, not null, default, identity,
/// generated, collation (when not its type's), its number, an enum type's labels
/// ([`ENUM_LABELS`]).
pub fn columns(version: u32) -> String {
    let identity = if supports(version, Feature::Identity) { "a.attidentity::text" } else { "''" };
    let generated =
        if supports(version, Feature::Generated) { "a.attgenerated::text" } else { "''" };
    format!(
        "SELECT a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), \
         a.attnotnull::text, pg_catalog.pg_get_expr(d.adbin, d.adrelid), {identity}, \
         {generated}, CASE WHEN a.attcollation <> t.typcollation THEN co.collname::text END, \
         a.attnum::text, {ENUM_LABELS} \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         LEFT JOIN pg_catalog.pg_collation co ON co.oid = a.attcollation \
         WHERE a.attrelid = $1::text::regclass AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum"
    )
}

/// The key columns an index has (its `INCLUDE`d ones left out, 11+).
fn key_count(version: u32) -> &'static str {
    if supports(version, Feature::Procedures) { "i.indnkeyatts" } else { "i.indnatts" }
}

/// A table's indexes (`$1`: its quoted name): name, unique, primary, made by a constraint,
/// predicate, each column's options (`indoption`).
pub fn indexes() -> String {
    "SELECT c.relname::text, i.indisunique::text, i.indisprimary::text, \
     (SELECT count(*) FROM pg_catalog.pg_constraint k WHERE k.conindid = i.indexrelid)::text, \
     pg_catalog.pg_get_expr(i.indpred, i.indrelid), i.indoption::text \
     FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indexrelid \
     WHERE i.indrelid = $1::text::regclass ORDER BY 1"
        .to_owned()
}

/// Each index's key columns in order (`$1`: the table's quoted name): the index's name, the
/// column's place (1-based), its name or expression.
pub fn index_columns(version: u32) -> String {
    format!(
        "SELECT x.name, x.k::text, pg_catalog.pg_get_indexdef(x.oid, x.k, true) FROM \
         (SELECT c.relname::text AS name, i.indexrelid AS oid, \
         generate_series(1, {}) AS k \
         FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indexrelid \
         WHERE i.indrelid = $1::text::regclass) x ORDER BY 1, 2",
        key_count(version)
    )
}

/// A table's foreign keys (`$1`: its quoted name): name, on update, on delete, the
/// referenced table's schema and name, the columns' numbers there and here, its oid.
pub fn foreign_keys() -> String {
    "SELECT k.conname::text, k.confupdtype::text, k.confdeltype::text, fn.nspname::text, \
     fc.relname::text, k.conkey::text, k.confkey::text, k.confrelid::text \
     FROM pg_catalog.pg_constraint k \
     JOIN pg_catalog.pg_class fc ON fc.oid = k.confrelid \
     JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
     WHERE k.contype = 'f' AND k.conrelid = $1::text::regclass ORDER BY 1"
        .to_owned()
}

/// A relation's column names by number (`$1`: its oid).
pub fn column_names() -> String {
    "SELECT a.attnum::text, a.attname::text FROM pg_catalog.pg_attribute a \
     WHERE a.attrelid = $1::text::oid AND a.attnum > 0 AND NOT a.attisdropped"
        .to_owned()
}

/// The relation kinds the console's completion knows (as [`tables`] lists them, partitions
/// included: a query may name one).
fn console_kinds(version: u32) -> String {
    let mut kinds = vec!["'r'", "'v'", "'f'"];
    if version >= 90300 {
        kinds.push("'m'");
    }
    if supports(version, Feature::Identity) {
        kinds.push("'p'");
    }
    kinds.join(", ")
}

/// Every column of every table and view (of one schema: `$1`), in one query: schema,
/// table, column, type, nullable, default (none for a generated column, 12+), comment, its
/// place in the primary key (from 1; `INCLUDE`d columns, 11+, aren't in it), an enum
/// type's labels (in their order, separated by U+001F: [`enum_labels`]). No
/// `array_position` (9.5+): the key's places from `generate_series`.
pub fn catalog_columns(version: u32, one_schema: bool) -> String {
    let default = if supports(version, Feature::Generated) {
        "CASE WHEN a.attgenerated = '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END"
    } else {
        "pg_catalog.pg_get_expr(d.adbin, d.adrelid)"
    };
    let mut sql = format!(
        "SELECT n.nspname::text, c.relname::text, a.attname::text, \
         pg_catalog.format_type(a.atttypid, a.atttypmod), (NOT a.attnotnull)::text, {default}, \
         pg_catalog.col_description(c.oid, a.attnum), \
         (SELECT (s.k + 1)::text FROM pg_catalog.generate_series(0, {keys} - 1) AS s(k) \
         WHERE i.indkey[s.k] = a.attnum), \
         {ENUM_LABELS} \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         LEFT JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary \
         WHERE c.relkind IN ({kinds}) AND a.attnum > 0 AND NOT a.attisdropped AND {SYSTEM}",
        keys = key_count(version),
        kinds = console_kinds(version),
    );
    if one_schema {
        sql.push_str(" AND n.nspname = $1");
    }
    sql.push_str(" ORDER BY 1, 2, a.attnum");
    sql
}

/// Every foreign key of every table (of one schema: `$1`), in one query, a row per column
/// in its order: schema, table, key name, the target's schema and name, the column here,
/// the column there, its place, on update, on delete. No `LATERAL` (9.3+), no
/// `WITH ORDINALITY` (9.4+): the places from a constant series (a key has at most 32
/// columns).
pub fn catalog_keys(one_schema: bool) -> String {
    let mut sql = format!(
        "SELECT n.nspname::text, c.relname::text, k.conname::text, fn.nspname::text, \
         fc.relname::text, a.attname::text, fa.attname::text, s.i::text, \
         k.confupdtype::text, k.confdeltype::text \
         FROM pg_catalog.pg_constraint k \
         JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_class fc ON fc.oid = k.confrelid \
         JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
         JOIN pg_catalog.generate_series(1, 32) AS s(i) \
         ON s.i <= pg_catalog.array_upper(k.conkey, 1) \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = k.conkey[s.i] \
         JOIN pg_catalog.pg_attribute fa ON fa.attrelid = k.confrelid \
         AND fa.attnum = k.confkey[s.i] \
         WHERE k.contype = 'f' AND {SYSTEM}"
    );
    if one_schema {
        sql.push_str(" AND n.nspname = $1");
    }
    sql.push_str(" ORDER BY 1, 2, 3, s.i");
    sql
}

/// The functions and procedures (of one schema: `$1`; else every one, the server's own in
/// `pg_catalog` too): schema, name, arguments, result, kind (`f`, `p`, `a`, `w`: `prokind`
/// from 11, `proisagg` / `proiswindow` before), the number of arguments, variadic, the
/// server's own.
pub fn routines(version: u32, one_schema: bool) -> String {
    let kind = if supports(version, Feature::Procedures) {
        "p.prokind::text"
    } else {
        "CASE WHEN p.proisagg THEN 'a' WHEN p.proiswindow THEN 'w' ELSE 'f' END"
    };
    let mut sql = format!(
        "SELECT n.nspname::text, p.proname::text, pg_catalog.pg_get_function_arguments(p.oid), \
         pg_catalog.pg_get_function_result(p.oid), {kind}, p.pronargs::text, \
         (p.provariadic <> 0)::text, (n.nspname = 'pg_catalog')::text \
         FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname <> 'information_schema' AND n.nspname NOT LIKE 'pg\\_toast%' \
         AND n.nspname NOT LIKE 'pg\\_temp%'"
    );
    if one_schema {
        sql.push_str(" AND n.nspname = $1");
    }
    sql.push_str(" ORDER BY 2, 1");
    sql
}

/// The databases a session may open: no templates, none refusing connections, none the
/// user may not connect to (`CONNECT` revoked from it).
pub fn databases() -> &'static str {
    "SELECT d.datname::text FROM pg_catalog.pg_database d \
     WHERE NOT d.datistemplate AND d.datallowconn \
     AND pg_catalog.has_database_privilege(d.oid, 'CONNECT') ORDER BY 1"
}

/// The labels of column `a`'s type when it's an enum (none otherwise), in their order,
/// separated by U+001F (9.1+: `enumsortorder`): [`enum_labels`] reads them.
const ENUM_LABELS: &str = "(SELECT pg_catalog.string_agg(e.enumlabel::text, pg_catalog.chr(31) \
     ORDER BY e.enumsortorder) FROM pg_catalog.pg_enum e WHERE e.enumtypid = a.atttypid)";

/// An enum's labels as [`catalog_columns`] reads them (separated by U+001F), in their order.
pub fn enum_labels(text: &str) -> Vec<String> {
    text.split('\u{1f}').map(str::to_owned).collect()
}

/// The numbers of a Postgres array or vector's text (`{1,2}`, `1 2`).
pub fn numbers(text: &str) -> Vec<i64> {
    text.trim_matches(|c| c == '{' || c == '}')
        .split([',', ' '])
        .filter_map(|n| n.trim().parse().ok())
        .collect()
}

/// A name as `pg_get_indexdef` gives it (quoted when it must be): unquoted.
pub fn unquote(name: &str) -> String {
    match name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) {
        Some(inner) => inner.replace("\"\"", "\""),
        None => name.to_owned(),
    }
}

/// A foreign key's action from its catalog letter.
pub fn action(letter: &str) -> database::FkAction {
    use database::FkAction::*;
    match letter {
        "r" => Restrict,
        "c" => Cascade,
        "n" => SetNull,
        "d" => SetDefault,
        _ => NoAction,
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
