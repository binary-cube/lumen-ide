//! What SQLite's catalog says of a database: its schemas (`main`, `temp`, the attached
//! ones), their tables and views with a row estimate when `ANALYZE` left one
//! (`sqlite_stat1`: nothing is counted to list them), a table's structure from the
//! `table_xinfo`, `index_list` / `index_xinfo` and `foreign_key_list` pragmas (the columns'
//! collations and generated expressions from the table's definition), and what a rebuild
//! needs beyond it, read from `sqlite_schema`: the `CHECK` constraints of the table's
//! definition, its triggers, its indexes on expressions or with a `WHERE`, and the views
//! naming it. Every name is a bound
//! parameter, never written into the SQL.

use std::collections::HashMap;

use database::sqlite::{RebuildSource, SqliteTable};
use database::{
    ColumnBrief, ColumnInfo, DbError, FkAction, ForeignKey, Generated, IndexInfo, Routine,
    RoutineKind, SchemaObject, Structure, TableColumns, TableInfo, TableKeys, TableKind, TableRef,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error;

/// The main database's schema: tables in it are named without one.
pub const MAIN: &str = "main";

/// `table`'s schema (`main` when it has none).
pub fn schema_of(table: &TableRef) -> &str {
    table.schema.as_deref().unwrap_or(MAIN)
}

/// A table of `schema`, as the domain names it (none for `main`'s).
pub fn table_ref(schema: &str, name: &str) -> TableRef {
    if schema.eq_ignore_ascii_case(MAIN) {
        TableRef::new(name)
    } else {
        TableRef::in_schema(schema, name)
    }
}

/// The schemas, in SQLite's order (`main`, `temp`, then those attached).
pub fn schemas(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt =
        conn.prepare("SELECT name FROM pragma_database_list ORDER BY seq").map_err(error)?;
    let names = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(error)?;
    names.collect::<Result<_, _>>().map_err(error)
}

/// `schema`'s tables (and virtual tables) and views, by name — SQLite's own (`sqlite_…`)
/// and the shadow tables of virtual ones left out.
pub fn tables(conn: &Connection, schema: &str) -> Result<Vec<TableInfo>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM pragma_table_list WHERE schema = ?1 \
             AND type IN ('table', 'view', 'virtual') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name COLLATE NOCASE",
        )
        .map_err(error)?;
    let rows = stmt
        .query_map(params![schema], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(error)?;
    let estimates = estimates(conn, schema)?;
    let mut tables = Vec::new();
    for row in rows {
        let (name, kind) = row.map_err(error)?;
        let kind = if kind == "view" { TableKind::View } else { TableKind::Table };
        let rows = estimates.get(&name.to_ascii_lowercase()).copied();
        tables.push(TableInfo { table: table_ref(schema, &name), kind, rows });
    }
    Ok(tables)
}

/// Every column of `schema`'s tables and views in one query (`pragma_table_list` joined
/// with each one's `pragma_table_info`), in their order; a schema where that fails (a view
/// naming a table gone, a virtual table of a module not here) is read a table at a time,
/// those that fail left out.
pub fn catalog_columns(conn: &Connection, schema: &str) -> Result<Vec<TableColumns>, DbError> {
    const BULK: &str = "SELECT m.name, p.name, p.type, p.\"notnull\", p.dflt_value, p.pk \
         FROM pragma_table_list AS m JOIN pragma_table_info(m.name, m.schema) AS p \
         WHERE m.schema = ?1 AND m.type IN ('table', 'view', 'virtual') \
         AND m.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY m.name COLLATE NOCASE, p.cid";
    let bulk = || -> rusqlite::Result<Vec<TableColumns>> {
        let mut stmt = conn.prepare(BULK)?;
        let mut rows = stmt.query(params![schema])?;
        let mut all: Vec<TableColumns> = Vec::new();
        while let Some(r) = rows.next()? {
            let table: String = r.get(0)?;
            let column = brief(r, 1)?;
            match all.last_mut() {
                Some((last, columns)) if last.name == table => columns.push(column),
                _ => all.push((table_ref(schema, &table), vec![column])),
            }
        }
        Ok(all)
    };
    match bulk() {
        Ok(all) => Ok(all),
        Err(_) => {
            let mut all = Vec::new();
            for table in tables(conn, schema)? {
                let mut stmt = conn
                    .prepare(
                        "SELECT name, type, \"notnull\", dflt_value, pk \
                         FROM pragma_table_info(?1, ?2) ORDER BY cid",
                    )
                    .map_err(error)?;
                let columns = stmt
                    .query_map(params![table.table.name, schema], |r| brief(r, 0))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>());
                if let Ok(columns) = columns {
                    all.push((table.table, columns));
                }
            }
            Ok(all)
        }
    }
}

/// Every table's foreign keys in `schema`, in one query (`pragma_table_list` ⋈
/// `pragma_foreign_key_list`); where that fails (an old SQLite), table by table. A key
/// naming no column (`REFERENCES t`) keeps its references empty: the target's primary key.
pub fn catalog_keys(conn: &Connection, schema: &str) -> Result<Vec<TableKeys>, DbError> {
    const BULK: &str = "SELECT m.name, f.id, f.\"table\", f.\"from\", f.\"to\", f.on_update, \
         f.on_delete FROM pragma_table_list AS m JOIN pragma_foreign_key_list(m.name, m.schema) AS f \
         WHERE m.schema = ?1 AND m.type = 'table' ORDER BY m.name COLLATE NOCASE, f.id, f.seq";
    let bulk = || -> rusqlite::Result<Vec<TableKeys>> {
        let mut stmt = conn.prepare(BULK)?;
        let mut rows = stmt.query(params![schema])?;
        let mut all: Vec<(TableRef, Vec<(i64, ForeignKey)>)> = Vec::new();
        while let Some(r) = rows.next()? {
            let table = table_ref(schema, &r.get::<_, String>(0)?);
            let id: i64 = r.get(1)?;
            if all.last().is_none_or(|(t, _)| *t != table) {
                all.push((table, Vec::new()));
            }
            let Some((_, keys)) = all.last_mut() else { continue };
            if keys.last().is_none_or(|(k, _)| *k != id) {
                keys.push((
                    id,
                    ForeignKey {
                        name: None,
                        columns: Vec::new(),
                        table: table_ref(schema, &r.get::<_, String>(2)?),
                        references: Vec::new(),
                        on_update: fk_action(&r.get::<_, String>(5)?),
                        on_delete: fk_action(&r.get::<_, String>(6)?),
                    },
                ));
            }
            if let Some((_, key)) = keys.last_mut() {
                key.columns.push(r.get(3)?);
                if let Some(to) = r.get::<_, Option<String>>(4)? {
                    key.references.push(to);
                }
            }
        }
        Ok(all
            .into_iter()
            .map(|(t, keys)| {
                let keys = keys
                    .into_iter()
                    .map(|(_, mut k)| {
                        if k.references.len() != k.columns.len() {
                            k.references.clear();
                        }
                        k
                    })
                    .collect();
                (t, keys)
            })
            .collect())
    };
    match bulk() {
        Ok(all) => Ok(all),
        Err(_) => {
            let mut all = Vec::new();
            for table in tables(conn, schema)? {
                if table.kind != TableKind::Table {
                    continue;
                }
                if let Ok(keys) = foreign_keys(conn, &table.table)
                    && !keys.is_empty()
                {
                    all.push((table.table, keys));
                }
            }
            Ok(all)
        }
    }
}

/// A `pragma_table_info` row from column `at` on (name, type, notnull, default, pk).
fn brief(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<ColumnBrief> {
    let pk: i64 = r.get(at + 4)?;
    Ok(ColumnBrief {
        name: r.get(at)?,
        type_name: r.get::<_, Option<String>>(at + 1)?.unwrap_or_default(),
        nullable: r.get::<_, i64>(at + 2)? == 0,
        default: r.get(at + 3)?,
        comment: None,
        key_position: u16::try_from(pk).ok().filter(|p| *p > 0),
        values: None,
    })
}

/// The functions this SQLite has (`pragma_function_list`, 3.30+): each name and arity once
/// (it lists one per text encoding), the built-in ones said so.
pub fn routines(conn: &Connection) -> Result<Vec<Routine>, DbError> {
    if rusqlite::version_number() < 3_030_000 {
        return Err(DbError::Unsupported("listing functions before SQLite 3.30".to_owned()));
    }
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT name, builtin, type, narg FROM pragma_function_list \
             ORDER BY name, narg",
        )
        .map_err(error)?;
    let rows = stmt
        .query_map([], |r| {
            let kind = match r.get::<_, String>(2)?.as_str() {
                "a" => RoutineKind::Aggregate,
                "w" => RoutineKind::Window,
                _ => RoutineKind::Function,
            };
            Ok(Routine {
                schema: None,
                name: r.get(0)?,
                arguments: String::new(),
                result: None,
                kind,
                arity: u16::try_from(r.get::<_, i64>(3)?).ok(),
                builtin: r.get::<_, i64>(1)? != 0,
            })
        })
        .map_err(error)?;
    let mut routines: Vec<Routine> = rows.collect::<Result<_, _>>().map_err(error)?;
    routines.dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
    Ok(routines)
}

/// The row counts `ANALYZE` left in `schema`'s `sqlite_stat1` (by table, lowercase): the
/// first number of a table's statistics is its rows. None without it.
pub(crate) fn estimates(conn: &Connection, schema: &str) -> Result<HashMap<String, u64>, DbError> {
    let analyzed: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM pragma_table_list WHERE schema = ?1 AND name = 'sqlite_stat1'",
            params![schema],
            |r| r.get(0),
        )
        .optional()
        .map_err(error)?;
    let mut estimates = HashMap::new();
    if analyzed.is_none() {
        return Ok(estimates);
    }
    let sql = format!("SELECT tbl, stat FROM {}.sqlite_stat1", quote(schema));
    let mut stmt = conn.prepare(&sql).map_err(error)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
        .map_err(error)?;
    for row in rows {
        let (table, stat) = row.map_err(error)?;
        let Some(rows) = stat.and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok()) else {
            continue;
        };
        let entry = estimates.entry(table.to_ascii_lowercase()).or_insert(0);
        *entry = (*entry).max(rows);
    }
    Ok(estimates)
}

/// `name` as an SQL identifier (double quotes, its own doubled).
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// What `sqlite_schema` keeps of `table`: its kind and the SQL that made it.
fn definition(conn: &Connection, table: &TableRef) -> Result<(TableKind, String), DbError> {
    let sql = format!(
        "SELECT type, sql FROM {}.sqlite_schema WHERE name = ?1 COLLATE NOCASE \
         AND type IN ('table', 'view')",
        quote(schema_of(table))
    );
    let found: Option<(String, Option<String>)> = conn
        .query_row(&sql, params![table.name], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(error)?;
    match found {
        Some((kind, sql)) => {
            let kind = if kind == "view" { TableKind::View } else { TableKind::Table };
            Ok((kind, sql.unwrap_or_default()))
        }
        None => Err(DbError::Query(format!("no such table: {table}"))),
    }
}

/// `table`'s structure, from the catalog (see the module's doc).
pub fn structure(conn: &Connection, table: &TableRef) -> Result<Structure, DbError> {
    let schema = schema_of(table);
    let (kind, sql) = definition(conn, table)?;

    let mut stmt = conn
        .prepare(
            "SELECT name, type, \"notnull\", dflt_value, pk, hidden \
             FROM pragma_table_xinfo(?1, ?2) ORDER BY cid",
        )
        .map_err(error)?;
    let rows = stmt
        .query_map(params![table.name, schema], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(error)?;
    let clauses = if kind == TableKind::Table { column_clauses(&sql) } else { Vec::new() };
    let mut columns = Vec::new();
    for row in rows {
        let (name, declared, not_null, default, pk, hidden) = row.map_err(error)?;
        // A virtual table's hidden columns aren't the table's (`SELECT *` leaves them out).
        if hidden == 1 {
            continue;
        }
        let clause = clauses.iter().find(|c| c.name.eq_ignore_ascii_case(&name));
        let mut column = ColumnInfo::new(name, declared);
        column.nullable = !not_null;
        column.default = default;
        column.in_key = pk > 0;
        column.key_position = u16::try_from(pk).ok().filter(|p| *p > 0);
        column.collation = clause.and_then(|c| c.collation.clone());
        // 2: generated, virtual; 3: generated, stored. Its expression from the definition
        // (none found: an empty one, which a rebuild refuses rather than lose it).
        if hidden == 2 || hidden == 3 {
            column.generated = Some(Generated {
                expression: clause.and_then(|c| c.generated.clone()).unwrap_or_default(),
                stored: hidden == 3,
            });
        }
        columns.push(column);
    }

    let rowid = match kind {
        TableKind::View => false,
        TableKind::Table => {
            let without: Option<i64> = conn
                .query_row(
                    "SELECT wr FROM pragma_table_list WHERE schema = ?1 AND name = ?2 \
                     COLLATE NOCASE",
                    params![schema, table.name],
                    |r| r.get(0),
                )
                .optional()
                .map_err(error)?;
            without != Some(1)
        }
    };
    // `AUTOINCREMENT` is only allowed on an `INTEGER PRIMARY KEY`: the table's one key.
    if rowid && has_word(&sql, "AUTOINCREMENT") {
        let key: Vec<usize> =
            columns.iter().enumerate().filter(|(_, c)| c.in_key).map(|(i, _)| i).collect();
        if let [at] = key[..]
            && columns[at].declared_type.trim().eq_ignore_ascii_case("integer")
        {
            columns[at].auto_increment = true;
        }
    }

    let (indexes, _) =
        if kind == TableKind::Table { indexes(conn, table)? } else { Default::default() };
    let foreign_keys =
        if kind == TableKind::Table { foreign_keys(conn, table)? } else { Vec::new() };
    Ok(Structure { table: table.clone(), columns, indexes, foreign_keys, rowid, lines: false })
}

/// `table`'s indexes, by name, and the names of those `CREATE INDEX` made on expressions or
/// with a `WHERE` (made again from their SQL, not from their columns, by a rebuild).
fn indexes(conn: &Connection, table: &TableRef) -> Result<(Vec<IndexInfo>, Vec<String>), DbError> {
    let schema = schema_of(table);
    let mut stmt = conn
        .prepare(
            "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1, ?2) \
             ORDER BY name COLLATE NOCASE",
        )
        .map_err(error)?;
    let listed = stmt
        .query_map(params![table.name, schema], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? != 0,
            ))
        })
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    let mut columns_of = conn
        .prepare(
            "SELECT cid, name, \"desc\" FROM pragma_index_xinfo(?1, ?2) WHERE key = 1 \
             ORDER BY seqno",
        )
        .map_err(error)?;
    let mut sql_of = conn
        .prepare(&format!(
            "SELECT sql FROM {}.sqlite_schema WHERE type = 'index' AND name = ?1",
            quote(schema)
        ))
        .map_err(error)?;
    let mut indexes = Vec::new();
    let mut as_written = Vec::new();
    for (name, unique, origin, partial) in listed {
        let key = columns_of
            .query_map(params![name, schema], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)? != 0))
            })
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        // An expression's column is -2 (and has no name).
        let expression = key.iter().any(|(cid, _, _)| *cid == -2);
        let implicit = origin != "c";
        if !implicit && (partial || expression) {
            as_written.push(name.clone());
        }
        // A partial index's condition, from its own SQL.
        let predicate = if partial {
            sql_of
                .query_row(params![name], |r| r.get::<_, Option<String>>(0))
                .ok()
                .flatten()
                .and_then(|sql| database::sqlite::index_predicate(&sql))
        } else {
            None
        };
        // All ascending (the usual): said by none.
        let descending: Vec<bool> =
            key.iter().filter(|(_, n, _)| n.is_some()).map(|(_, _, d)| *d).collect();
        let descending = if descending.contains(&true) { descending } else { Vec::new() };
        indexes.push(IndexInfo {
            name,
            columns: key.into_iter().filter_map(|(_, name, _)| name).collect(),
            unique,
            implicit,
            descending,
            predicate,
        });
    }
    Ok((indexes, as_written))
}

fn fk_action(text: &str) -> FkAction {
    match text.to_ascii_uppercase().as_str() {
        "RESTRICT" => FkAction::Restrict,
        "CASCADE" => FkAction::Cascade,
        "SET NULL" => FkAction::SetNull,
        "SET DEFAULT" => FkAction::SetDefault,
        _ => FkAction::NoAction,
    }
}

/// The primary key's columns of `table` (in the key's order).
fn primary_key(conn: &Connection, schema: &str, table: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1, ?2) WHERE pk > 0 ORDER BY pk")
        .map_err(error)?;
    let names = stmt.query_map(params![table, schema], |r| r.get::<_, String>(0)).map_err(error)?;
    names.collect::<Result<_, _>>().map_err(error)
}

/// `table`'s foreign keys (to tables of its own schema: SQLite's can't point elsewhere,
/// hence no schema on them). One pointing to its parent's key without naming it names
/// those columns.
fn foreign_keys(conn: &Connection, table: &TableRef) -> Result<Vec<ForeignKey>, DbError> {
    let schema = schema_of(table);
    let mut stmt = conn
        .prepare(
            "SELECT id, \"table\", \"from\", \"to\", on_update, on_delete \
             FROM pragma_foreign_key_list(?1, ?2) ORDER BY id, seq",
        )
        .map_err(error)?;
    let rows = stmt
        .query_map(params![table.name, schema], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    let mut keys: Vec<(i64, ForeignKey, bool)> = Vec::new();
    for (id, parent, from, to, on_update, on_delete) in rows {
        let at = match keys.iter().position(|(k, _, _)| *k == id) {
            Some(at) => at,
            None => {
                let fk = ForeignKey {
                    name: None,
                    columns: Vec::new(),
                    table: TableRef::new(parent),
                    references: Vec::new(),
                    on_update: fk_action(&on_update),
                    on_delete: fk_action(&on_delete),
                };
                keys.push((id, fk, false));
                keys.len() - 1
            }
        };
        let (_, fk, implied) = &mut keys[at];
        fk.columns.push(from);
        match to {
            Some(to) => fk.references.push(to),
            None => *implied = true,
        }
    }
    let mut out = Vec::with_capacity(keys.len());
    for (_, mut fk, implied) in keys {
        if implied {
            fk.references = primary_key(conn, schema, &fk.table.name)?;
        }
        out.push(fk);
    }
    Ok(out)
}

/// What rebuilding `table` needs (see the module's doc), and whether foreign keys are on.
/// The indexes made on expressions or with a `WHERE` are made again from their own SQL
/// (their columns alone would lose what they index).
pub fn rebuild_source(conn: &Connection, table: &TableRef) -> Result<RebuildSource, DbError> {
    let schema = schema_of(table);
    let (kind, sql) = definition(conn, table)?;
    if kind == TableKind::View {
        return Err(DbError::Unsupported(format!("rebuilding the view {table}")));
    }
    let mut structure = structure(conn, table)?;
    let (_, as_written) = indexes(conn, table)?;
    structure.indexes.retain(|i| !as_written.contains(&i.name));

    let objects = format!(
        "SELECT type, name, tbl_name, sql FROM {}.sqlite_schema \
         WHERE type IN ('trigger', 'view', 'index') AND sql IS NOT NULL ORDER BY rowid",
        quote(schema)
    );
    let mut stmt = conn.prepare(&objects).map_err(error)?;
    let all = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;

    let mut triggers = Vec::new();
    let mut written_indexes = Vec::new();
    for (kind, name, owner, sql) in &all {
        if !owner.eq_ignore_ascii_case(&table.name) {
            continue;
        }
        let object = SchemaObject { name: name.clone(), sql: sql.clone() };
        if kind == "trigger" {
            triggers.push(object);
        } else if kind == "index" && as_written.contains(name) {
            written_indexes.push(object);
        }
    }
    // The views naming the table, and those naming them (each made after what it reads).
    let views: Vec<&(String, String, String, String)> =
        all.iter().filter(|(kind, ..)| kind == "view").collect();
    let mut named = vec![table.name.clone()];
    let mut taken = vec![false; views.len()];
    loop {
        let mut more = false;
        for (at, view) in views.iter().enumerate() {
            let reads = |w: &String| named.iter().any(|n| w.eq_ignore_ascii_case(n));
            if !taken[at] && names(&view.3).any(|w| reads(&w)) {
                taken[at] = true;
                named.push(view.1.clone());
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    let views = views
        .iter()
        .zip(&taken)
        .filter(|(_, taken)| **taken)
        .map(|(v, _)| SchemaObject { name: v.1.clone(), sql: v.3.clone() })
        .collect();

    let foreign_keys: i64 =
        conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).map_err(error)?;
    Ok(RebuildSource {
        table: SqliteTable { structure, checks: checks(&sql), triggers, written_indexes, views },
        foreign_keys: foreign_keys != 0,
    })
}

/// A token of SQL text: what the scanner tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// A keyword or a bare name.
    Word,
    /// A quoted name (`"…"`, `` `…` ``, `[…]`).
    Quoted,
    /// A string (`'…'`).
    Text,
    Open,
    Close,
    Other,
}

/// `sql`'s tokens with their byte ranges (comments and spaces left out).
fn tokens(sql: &str) -> Vec<(Token, usize, usize)> {
    let b = sql.as_bytes();
    let closing = |from: usize, quote: u8| -> usize {
        let mut i = from;
        while i < b.len() {
            if b[i] == quote {
                // A quote doubled is one quote, inside.
                if b.get(i + 1) == Some(&quote) {
                    i += 2;
                    continue;
                }
                return i + 1;
            }
            i += 1;
        }
        b.len()
    };
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        match c {
            b'\'' => {
                i = closing(i + 1, c);
                out.push((Token::Text, start, i));
            }
            b'"' | b'`' => {
                i = closing(i + 1, c);
                out.push((Token::Quoted, start, i));
            }
            b'[' => {
                i = b[i..].iter().position(|&c| c == b']').map_or(b.len(), |at| i + at + 1);
                out.push((Token::Quoted, start, i));
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |at| i + at + 1);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = sql[i + 2..].find("*/").map_or(b.len(), |at| i + 2 + at + 2);
            }
            b'(' => {
                i += 1;
                out.push((Token::Open, start, i));
            }
            b')' => {
                i += 1;
                out.push((Token::Close, start, i));
            }
            c if c.is_ascii_whitespace() => i += 1,
            c if word(c) => {
                while i < b.len() && word(b[i]) {
                    i += 1;
                }
                out.push((Token::Word, start, i));
            }
            _ => {
                i += 1;
                out.push((Token::Other, start, i));
            }
        }
    }
    out
}

/// `sql` has the keyword `word` (outside strings, quotes and comments).
pub fn has_word(sql: &str, word: &str) -> bool {
    tokens(sql).iter().any(|&(t, s, e)| t == Token::Word && sql[s..e].eq_ignore_ascii_case(word))
}

/// The names `sql` mentions: its words and quoted names (unquoted).
pub fn names(sql: &str) -> impl Iterator<Item = String> + '_ {
    tokens(sql).into_iter().filter_map(move |(t, s, e)| match t {
        Token::Word => Some(sql[s..e].to_owned()),
        Token::Quoted if e - s >= 2 => {
            let inner = &sql[s + 1..e - 1];
            Some(match sql.as_bytes()[s] {
                b'"' => inner.replace("\"\"", "\""),
                b'`' => inner.replace("``", "`"),
                _ => inner.to_owned(),
            })
        }
        _ => None,
    })
}

/// What a column's definition in a `CREATE TABLE` says beyond the pragmas: its collation
/// and a generated column's expression (as written).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnClause {
    pub name: String,
    pub collation: Option<String>,
    pub generated: Option<String>,
}

/// A name token's text, unquoted.
fn unquote(sql: &str, (t, s, e): (Token, usize, usize)) -> Option<String> {
    match t {
        Token::Word => Some(sql[s..e].to_owned()),
        Token::Quoted if e - s >= 2 => {
            let inner = &sql[s + 1..e - 1];
            Some(match sql.as_bytes()[s] {
                b'"' => inner.replace("\"\"", "\""),
                b'`' => inner.replace("``", "`"),
                _ => inner.to_owned(),
            })
        }
        _ => None,
    }
}

/// The columns' clauses of a `CREATE TABLE` (its table constraints left out): each
/// column's `COLLATE` and `[GENERATED ALWAYS] AS (…)`, at the definition's own level (not
/// inside a `CHECK` or a default).
pub fn column_clauses(sql: &str) -> Vec<ColumnClause> {
    let tokens = tokens(sql);
    // The definitions: what's between the first `(` and its `)`, split at its commas.
    let Some(open) = tokens.iter().position(|t| t.0 == Token::Open) else { return Vec::new() };
    let mut parts: Vec<Vec<(Token, usize, usize)>> = vec![Vec::new()];
    let mut depth = 0usize;
    for &token in &tokens[open + 1..] {
        match token.0 {
            Token::Open => depth += 1,
            Token::Close if depth == 0 => break,
            Token::Close => depth -= 1,
            Token::Other if depth == 0 && &sql[token.1..token.2] == "," => {
                parts.push(Vec::new());
                continue;
            }
            _ => {}
        }
        if let Some(part) = parts.last_mut() {
            part.push(token);
        }
    }
    let word = |t: &(Token, usize, usize), w: &str| {
        t.0 == Token::Word && sql[t.1..t.2].eq_ignore_ascii_case(w)
    };
    let mut clauses = Vec::new();
    for part in parts {
        let Some(first) = part.first() else { continue };
        if ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"].iter().any(|w| word(first, w)) {
            continue;
        }
        let Some(name) = unquote(sql, *first) else { continue };
        let mut clause = ColumnClause { name, ..ColumnClause::default() };
        let mut depth = 0usize;
        let mut i = 1;
        while i < part.len() {
            let token = part[i];
            match token.0 {
                Token::Open => depth += 1,
                Token::Close => depth = depth.saturating_sub(1),
                _ if depth > 0 => {}
                _ if word(&token, "COLLATE") => {
                    clause.collation = part.get(i + 1).and_then(|t| unquote(sql, *t));
                }
                _ if word(&token, "AS") && part.get(i + 1).is_some_and(|t| t.0 == Token::Open) => {
                    // The expression: up to its own `)`.
                    let mut inner = 0usize;
                    let mut j = i + 1;
                    while j < part.len() {
                        match part[j].0 {
                            Token::Open => inner += 1,
                            Token::Close => {
                                inner -= 1;
                                if inner == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    let end = part.get(j).map_or(sql.len(), |t| t.1);
                    clause.generated = Some(sql[part[i + 1].2..end].trim().to_owned());
                    i = j + 1;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        clauses.push(clause);
    }
    clauses
}

/// The `CHECK` constraints of a `CREATE TABLE` (of its columns and of the table), their
/// expressions as written.
pub fn checks(sql: &str) -> Vec<String> {
    let tokens = tokens(sql);
    let mut checks = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        let (t, s, e) = tokens[i];
        match t {
            Token::Open => depth += 1,
            Token::Close => depth = depth.saturating_sub(1),
            Token::Word
                if depth == 1
                    && sql[s..e].eq_ignore_ascii_case("CHECK")
                    && tokens.get(i + 1).is_some_and(|t| t.0 == Token::Open) =>
            {
                let open = tokens[i + 1];
                let mut inner = 0usize;
                let mut j = i + 1;
                while j < tokens.len() {
                    match tokens[j].0 {
                        Token::Open => inner += 1,
                        Token::Close => {
                            inner -= 1;
                            if inner == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let end = tokens.get(j).map_or(sql.len(), |t| t.1);
                checks.push(sql[open.2..end].trim().to_owned());
                i = j + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    checks
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
