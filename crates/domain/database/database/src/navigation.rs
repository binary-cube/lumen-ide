//! What the name at the cursor of a console's statement is, through the source's schema
//! ([`SchemaIndex`]), for "Go to Definition" and the hover card: a table of the database
//! (named in `FROM`, or through its alias), one of its columns (qualified, or found in the
//! only table in scope having it), or one of the statement's own tables (a CTE, a derived
//! table, a function's rows) and its columns — with where it's defined in the text.
//!
//! Pure and cheap: the statement's names read once ([`crate::sql::resolve`]), the index
//! asked by binary search. Columns not read yet (a huge schema's second tier) are said
//! ([`Lookup::missing`]): asked for in the background.

use std::ops::Range;

use crate::TableRef;
use crate::schema_index::SchemaIndex;
use crate::sql::Flavor;
use crate::sql::lex::Token;
use crate::sql::resolve::{LevelScope, NameRole, Resolution, resolve_at};
use crate::sql::scope::{OutputColumn, ScopeTable, TableSource};

/// What a statement's own table is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    Cte,
    /// `(SELECT …) t`.
    Derived,
    /// `generate_series(1, 3) g`.
    Function,
}

/// What a name is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Table `table` of the index; `alias`: where the alias the name is was given (the name
    /// is a use of it).
    Table { table: usize, alias: Option<Range<usize>> },
    /// Column `column` of the index's table `table`.
    Column { table: usize, column: usize },
    /// One of the statement's own tables: where it's defined (a CTE's name, a derived
    /// table's `(`, a function), its columns as far as they're known.
    Local { name: String, kind: LocalKind, defined: Range<usize>, columns: Vec<String> },
    /// A column of one of the statement's own tables (`owner`), defined there.
    LocalColumn { column: String, owner: String, kind: LocalKind, defined: Range<usize> },
    /// An unqualified column several tables in scope have (`owners`, as the statement calls
    /// them).
    Ambiguous { column: String, owners: Vec<String> },
}

/// The name at the cursor and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The name (bytes of the text).
    pub range: Range<usize>,
    pub target: Target,
}

/// The name at `offset` of `text` (`tokens`: the whole text's) and what it is, with the
/// tables whose columns it needs and weren't read (`None`: not a name, or nothing known of
/// it).
pub fn lookup(
    text: &str,
    tokens: &[Token],
    offset: usize,
    flavor: Flavor,
    index: Option<&SchemaIndex>,
) -> (Option<Lookup>, Vec<TableRef>) {
    let (res, _) = resolve_at(text, tokens, offset, flavor);
    let mut missing = Vec::new();
    let found = res.name_at(offset).and_then(|name| {
        let target = target(&res, name, index, &mut missing)?;
        Some(Lookup { range: name.range.clone(), target })
    });
    (found, missing)
}

fn target(
    res: &Resolution,
    name: &crate::sql::Name,
    index: Option<&SchemaIndex>,
    missing: &mut Vec<TableRef>,
) -> Option<Target> {
    let scope = res.scopes.get(name.level)?;
    match &name.role {
        NameRole::Table { table } | NameRole::Alias { table } => {
            table_target(scope, *table, index, None)
        }
        NameRole::CteName => {
            let cte = scope.ctes.iter().find(|c| c.range == name.range)?;
            Some(Target::Local {
                name: cte.name.clone(),
                kind: LocalKind::Cte,
                defined: cte.range.clone(),
                columns: output_names(&cte.columns, index, missing),
            })
        }
        NameRole::Qualifier { parts } => {
            let [table] = res.qualified(name.level, parts)[..] else { return None };
            // A use of an alias: where it's given.
            let alias = (parts.len() == 1)
                .then(|| res.alias_of(name.level, table))
                .flatten()
                .filter(|a| a.text.eq_ignore_ascii_case(&parts[0]))
                .map(|a| a.range.clone());
            table_target(scope, table, index, alias)
        }
        NameRole::Column { qualifier, certain } => {
            if !qualifier.is_empty() {
                let [table] = res.qualified(name.level, qualifier)[..] else { return None };
                return column_target(scope, table, &name.text, index, missing);
            }
            if !certain {
                return None;
            }
            let owners = owners(scope, &name.text, index, missing);
            match owners.as_slice() {
                [] => None,
                [one] => column_target(scope, *one, &name.text, index, missing),
                // Merged by `USING` / `NATURAL`: the first.
                [first, ..] if scope.using => {
                    column_target(scope, *first, &name.text, index, missing)
                }
                many => Some(Target::Ambiguous {
                    column: name.text.clone(),
                    owners: many.iter().map(|&t| called(&scope.tables[t])).collect(),
                }),
            }
        }
        NameRole::TargetColumn { table } => {
            column_target(scope, *table, &name.text, index, missing)
        }
        NameRole::Schema { .. } | NameRole::Function { .. } => None,
    }
}

/// What table `t` of `scope` is (`alias`: where its alias was given, for a use of it).
fn table_target(
    scope: &LevelScope,
    t: usize,
    index: Option<&SchemaIndex>,
    alias: Option<Range<usize>>,
) -> Option<Target> {
    let st = scope.tables.get(t)?;
    let mut missing = Vec::new();
    match &st.source {
        TableSource::Table(r) => {
            let i = index?.find(r)?;
            Some(Target::Table { table: i, alias })
        }
        TableSource::Cte { name, columns } => {
            let defined = scope
                .ctes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
                .map_or_else(|| st.range.clone(), |c| c.range.clone());
            Some(Target::Local {
                name: name.clone(),
                kind: LocalKind::Cte,
                defined: alias.unwrap_or(defined),
                columns: output_names(columns, index, &mut missing),
            })
        }
        TableSource::Derived { columns } => Some(Target::Local {
            name: st.alias.clone().unwrap_or_default(),
            kind: LocalKind::Derived,
            defined: alias.unwrap_or(st.range.start..st.range.start + 1),
            columns: output_names(columns, index, &mut missing),
        }),
        TableSource::Function(name) => Some(Target::Local {
            name: st.alias.clone().unwrap_or_else(|| name.clone()),
            kind: LocalKind::Function,
            defined: alias.unwrap_or_else(|| st.range.clone()),
            columns: Vec::new(),
        }),
    }
}

/// Column `column` of table `t` of `scope`.
fn column_target(
    scope: &LevelScope,
    t: usize,
    column: &str,
    index: Option<&SchemaIndex>,
    missing: &mut Vec<TableRef>,
) -> Option<Target> {
    let st = scope.tables.get(t)?;
    let local = |kind, defined: Range<usize>| Target::LocalColumn {
        column: column.to_owned(),
        owner: called(st),
        kind,
        defined,
    };
    match &st.source {
        TableSource::Table(r) => {
            let index = index?;
            let i = index.find(r)?;
            if index.table(i).columns.is_none() {
                missing.push(index.table(i).table.clone());
                return None;
            }
            let c = index.column(i, column)?;
            Some(Target::Column { table: i, column: c })
        }
        TableSource::Cte { name, columns } => output_names(columns, index, missing)
            .iter()
            .any(|c| c.eq_ignore_ascii_case(column))
            .then(|| {
                let defined = scope
                    .ctes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name))
                    .map_or_else(|| st.range.clone(), |c| c.range.clone());
                local(LocalKind::Cte, defined)
            }),
        TableSource::Derived { columns } => output_names(columns, index, missing)
            .iter()
            .any(|c| c.eq_ignore_ascii_case(column))
            .then(|| local(LocalKind::Derived, st.range.start..st.range.start + 1)),
        TableSource::Function(_) => None,
    }
}

/// The tables in `scope` nearest to it having column `column` (by their level: an inner
/// one hides the outer ones), as far as their columns are known. None when a nearer table's
/// columns aren't known (it may be its).
fn owners(
    scope: &LevelScope,
    column: &str,
    index: Option<&SchemaIndex>,
    missing: &mut Vec<TableRef>,
) -> Vec<usize> {
    let deepest = scope.tables.iter().map(|t| t.level).max().unwrap_or(0);
    for depth in 0..=deepest {
        let mut found = Vec::new();
        let mut unknown = false;
        for (i, t) in scope.tables.iter().enumerate().filter(|(_, t)| t.level == depth) {
            match columns_of(t, index, missing) {
                Some(names) if names.iter().any(|n| n.eq_ignore_ascii_case(column)) => {
                    found.push(i);
                }
                Some(_) => {}
                None => unknown = true,
            }
        }
        if !found.is_empty() {
            return found;
        }
        if unknown {
            return Vec::new();
        }
    }
    Vec::new()
}

/// The columns of a table in scope (`None`: not known — a database table's not read yet,
/// a function's rows).
pub fn columns_of(
    t: &ScopeTable,
    index: Option<&SchemaIndex>,
    missing: &mut Vec<TableRef>,
) -> Option<Vec<String>> {
    match &t.source {
        TableSource::Table(r) => {
            let index = index?;
            let i = index.find(r)?;
            match &index.table(i).columns {
                Some(c) => Some(c.iter().map(|c| c.name.to_string()).collect()),
                None => {
                    missing.push(index.table(i).table.clone());
                    None
                }
            }
        }
        TableSource::Cte { columns, .. } | TableSource::Derived { columns } => {
            let complete = columns.iter().all(|c| match c {
                OutputColumn::Named(_) => true,
                OutputColumn::Star { tables, .. } => index.is_some_and(|index| {
                    tables
                        .iter()
                        .all(|r| index.find(r).is_some_and(|i| index.table(i).columns.is_some()))
                }),
            });
            let names = output_names(columns, index, missing);
            (complete && !names.is_empty()).then_some(names)
        }
        TableSource::Function(_) => None,
    }
}

/// The names of a CTE's or a derived table's columns (its `*`s through the index).
fn output_names(
    columns: &[OutputColumn],
    index: Option<&SchemaIndex>,
    missing: &mut Vec<TableRef>,
) -> Vec<String> {
    let mut out = Vec::new();
    for c in columns {
        match c {
            OutputColumn::Named(n) => out.push(n.clone()),
            OutputColumn::Star { tables, .. } => {
                let Some(index) = index else { continue };
                for r in tables {
                    let Some(i) = index.find(r) else { continue };
                    match &index.table(i).columns {
                        Some(c) => out.extend(c.iter().map(|c| c.name.to_string())),
                        None => missing.push(index.table(i).table.clone()),
                    }
                }
            }
        }
    }
    out
}

/// What the statement calls a table in scope: its alias, else its name.
pub fn called(t: &ScopeTable) -> String {
    match (&t.alias, &t.source) {
        (Some(a), _) => a.clone(),
        (None, TableSource::Table(r)) => r.name.clone(),
        (None, TableSource::Cte { name, .. } | TableSource::Function(name)) => name.clone(),
        (None, TableSource::Derived { .. }) => String::new(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/navigation.rs"]
mod tests;
