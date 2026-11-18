//! What a finding suggests ([`Suggestion`]): SQL ready to copy or put in the console — an
//! index, `ANALYZE`, `VACUUM`, extended statistics, a session's setting — written through the
//! source's dialect (every name quoted by it), or a rewrite of the statement said in words.
//! Never run by the editor: an index on a big table blocks its writes for as long as it's
//! made, a setting has costs the profile doesn't see; the user decides. On a production
//! PostgreSQL source an index is made `CONCURRENTLY` (which can't run in a transaction).

use crate::TableRef;
use crate::dialect::{Dialect, check_expression};
use crate::sql::Flavor;

/// PostgreSQL's longest name (63 bytes): an index's name is cut to it.
pub const MAX_NAME_BYTES: usize = 63;

/// A rewrite of the statement, said in words (no text generated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RewriteHint {
    /// `UNION ALL` of one query per branch of an `OR`.
    UnionAll,
    /// The next page found by the last row's key (`WHERE id > :last`), not `OFFSET`.
    KeysetPagination,
    /// Only the columns used, not `*`.
    ListColumns,
    /// One query for many values (`IN (…)`, a join), not one each.
    Batch,
    /// The value of the column's own type, so its index can be used.
    MatchLiteralType,
    /// The condition without the function around its column (or an index on it).
    UnwrapColumn,
}

impl RewriteHint {
    pub fn id(self) -> &'static str {
        match self {
            RewriteHint::UnionAll => "union_all",
            RewriteHint::KeysetPagination => "keyset_pagination",
            RewriteHint::ListColumns => "list_columns",
            RewriteHint::Batch => "batch",
            RewriteHint::MatchLiteralType => "match_literal_type",
            RewriteHint::UnwrapColumn => "unwrap_column",
        }
    }
}

/// What a finding suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggestion {
    CreateIndex {
        table: TableRef,
        columns: Vec<String>,
        /// Columns carried in the index (PostgreSQL 11+'s `INCLUDE`).
        include: Vec<String>,
        /// A partial index's condition.
        where_clause: Option<String>,
        /// An index on an expression (`lower(email)`) instead of columns.
        expression: Option<String>,
        /// The index's method and operator class (`gin` with `gin_trgm_ops`).
        method: Option<(String, String)>,
        concurrently: bool,
        sql: String,
    },
    Analyze {
        table: TableRef,
        sql: String,
    },
    Vacuum {
        table: TableRef,
        sql: String,
    },
    /// Extended statistics (`CREATE STATISTICS`, PostgreSQL 10+).
    Statistics {
        table: TableRef,
        columns: Vec<String>,
        sql: String,
    },
    /// A setting for the session only (`SET work_mem = '64MB'`).
    Setting {
        name: String,
        value: String,
        sql: String,
    },
    Rewrite {
        hint: RewriteHint,
    },
}

impl Suggestion {
    /// Its SQL (none for a rewrite).
    pub fn sql(&self) -> Option<&str> {
        match self {
            Suggestion::CreateIndex { sql, .. }
            | Suggestion::Analyze { sql, .. }
            | Suggestion::Vacuum { sql, .. }
            | Suggestion::Statistics { sql, .. }
            | Suggestion::Setting { sql, .. } => Some(sql),
            Suggestion::Rewrite { .. } => None,
        }
    }

    /// Stable: `create_index`, `analyze`…
    pub fn id(&self) -> &'static str {
        match self {
            Suggestion::CreateIndex { .. } => "create_index",
            Suggestion::Analyze { .. } => "analyze",
            Suggestion::Vacuum { .. } => "vacuum",
            Suggestion::Statistics { .. } => "statistics",
            Suggestion::Setting { .. } => "setting",
            Suggestion::Rewrite { .. } => "rewrite",
        }
    }
}

/// An index to suggest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexSpec {
    pub columns: Vec<String>,
    /// Which columns are in descending order, by place (missing ones: ascending).
    pub descending: Vec<bool>,
    pub include: Vec<String>,
    pub where_clause: Option<String>,
    pub expression: Option<String>,
    pub method: Option<(String, String)>,
}

impl IndexSpec {
    pub fn on(columns: Vec<String>) -> Self {
        Self { columns, ..Self::default() }
    }
}

/// An index's name: `idx_<table>_<columns>` (or the expression's words), only letters,
/// digits and `_`, lower case, cut to [`MAX_NAME_BYTES`].
pub fn index_name(table: &TableRef, spec: &IndexSpec) -> String {
    let mut words = vec!["idx".to_owned(), table.name.clone()];
    match &spec.expression {
        Some(e) => words.push(e.clone()),
        None => words.extend(spec.columns.iter().cloned()),
    }
    if let Some((method, _)) = &spec.method
        && method.eq_ignore_ascii_case("gin")
    {
        words.push("trgm".to_owned());
    }
    let mut name = String::new();
    for c in words.join("_").chars() {
        let c = c.to_ascii_lowercase();
        let c = if c.is_ascii_alphanumeric() { c } else { '_' };
        if c == '_' && name.ends_with('_') {
            continue;
        }
        name.push(c);
    }
    let name = name.trim_end_matches('_');
    let mut cut = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    name[..cut].trim_end_matches('_').to_owned()
}

/// `CREATE INDEX` for `spec` on `table`, written by `dialect` (`concurrently`: PostgreSQL's
/// form that doesn't block writes). `None`: a name it can't quote, an expression or a
/// condition that isn't one expression.
pub fn create_index(
    dialect: &dyn Dialect,
    table: &TableRef,
    spec: &IndexSpec,
    concurrently: bool,
) -> Option<Suggestion> {
    let name = index_name(table, spec);
    let sqlite = dialect.flavor() == Flavor::Sqlite;
    // SQLite puts the schema (an attached database) on the index, its table unqualified.
    let (index_sql, table_sql) = if sqlite {
        (dialect.index_name(table, &name).ok()?, dialect.quote_identifier(&table.name).ok()?)
    } else {
        (dialect.quote_identifier(&name).ok()?, dialect.table_name(table).ok()?)
    };
    let keys = match (&spec.expression, &spec.method) {
        (Some(expression), _) => format!("(({}))", check_expression(expression).ok()?),
        (None, Some((_, opclass))) => {
            let columns: Vec<String> = spec
                .columns
                .iter()
                .map(|c| dialect.quote_identifier(c).map(|q| format!("{q} {opclass}")))
                .collect::<Result<_, _>>()
                .ok()?;
            format!("({})", columns.join(", "))
        }
        (None, None) => {
            if spec.columns.is_empty() {
                return None;
            }
            let columns: Vec<String> = spec
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let q = dialect.quote_identifier(c)?;
                    Ok(if spec.descending.get(i) == Some(&true) { format!("{q} DESC") } else { q })
                })
                .collect::<Result<_, crate::DbError>>()
                .ok()?;
            format!("({})", columns.join(", "))
        }
    };
    let method = spec.method.as_ref().map_or(String::new(), |(m, _)| format!(" USING {m}"));
    let include = if spec.include.is_empty() || sqlite {
        String::new()
    } else {
        format!(" INCLUDE {}", dialect.name_list(&spec.include).ok()?)
    };
    let predicate = match spec.where_clause.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => format!(" WHERE {}", check_expression(p).ok()?),
        _ => String::new(),
    };
    let concurrently = concurrently && dialect.flavor().is_postgres();
    let sql = format!(
        "CREATE INDEX {}{index_sql} ON {table_sql}{method} {keys}{include}{predicate};",
        if concurrently { "CONCURRENTLY " } else { "" },
    );
    Some(Suggestion::CreateIndex {
        table: table.clone(),
        columns: spec.columns.clone(),
        include: if sqlite { Vec::new() } else { spec.include.clone() },
        where_clause: spec.where_clause.clone(),
        expression: spec.expression.clone(),
        method: spec.method.clone(),
        concurrently,
        sql,
    })
}

/// `ANALYZE table;`.
pub fn analyze(dialect: &dyn Dialect, table: &TableRef) -> Option<Suggestion> {
    let sql = format!("ANALYZE {};", dialect.table_name(table).ok()?);
    Some(Suggestion::Analyze { table: table.clone(), sql })
}

/// `VACUUM (ANALYZE) table;` (PostgreSQL: a table at a time; SQLite's is the whole file:
/// none).
pub fn vacuum(dialect: &dyn Dialect, table: &TableRef) -> Option<Suggestion> {
    if !dialect.flavor().is_postgres() {
        return None;
    }
    let sql = format!("VACUUM (ANALYZE) {};", dialect.table_name(table).ok()?);
    Some(Suggestion::Vacuum { table: table.clone(), sql })
}

/// `CREATE STATISTICS` on `columns` of `table` (PostgreSQL 10+; `mcv` from 12), then the
/// `ANALYZE` that fills them.
pub fn statistics(
    dialect: &dyn Dialect,
    version: Option<u32>,
    table: &TableRef,
    columns: &[String],
) -> Option<Suggestion> {
    if !dialect.flavor().is_postgres() || version.is_some_and(|v| v < 100_000) || columns.len() < 2
    {
        return None;
    }
    let kinds = if version.is_none_or(|v| v >= 120_000) {
        "ndistinct, dependencies, mcv"
    } else {
        "ndistinct, dependencies"
    };
    let mut name = index_name(table, &IndexSpec::on(columns.to_vec()));
    name.replace_range(..3, "stx");
    let table_sql = dialect.table_name(table).ok()?;
    let names: Vec<String> =
        columns.iter().map(|c| dialect.quote_identifier(c)).collect::<Result<_, _>>().ok()?;
    let sql = format!(
        "CREATE STATISTICS {} ({kinds}) ON {} FROM {table_sql};\nANALYZE {table_sql};",
        dialect.quote_identifier(&name).ok()?,
        names.join(", "),
    );
    Some(Suggestion::Statistics { table: table.clone(), columns: columns.to_vec(), sql })
}

/// `SET name = value;` for the session (`value` a word, a number or a quoted text; anything
/// else refused).
pub fn setting(name: &str, value: &str) -> Option<Suggestion> {
    let name_ok =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    let plain = !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !name_ok || !plain {
        return None;
    }
    let written = if value.chars().all(|c| c.is_ascii_digit()) || ["on", "off"].contains(&value) {
        value.to_owned()
    } else {
        format!("'{value}'")
    };
    Some(Suggestion::Setting {
        name: name.to_owned(),
        value: value.to_owned(),
        sql: format!("SET {name} = {written};"),
    })
}

/// A memory setting's value for at least `bytes`: the next power of two in MB (at least
/// 4 MB), as PostgreSQL writes it (`64MB`, `2GB`).
pub fn memory_value(bytes: u64) -> String {
    let mb = bytes.div_ceil(1 << 20).max(4).next_power_of_two();
    if mb >= 1024 && mb.is_multiple_of(1024) {
        format!("{}GB", mb / 1024)
    } else {
        format!("{mb}MB")
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/profile/rules/suggest.rs"]
mod tests;
