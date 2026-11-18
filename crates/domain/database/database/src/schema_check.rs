//! A console's text checked against its source's schema ([`SchemaIndex`]), reporting only
//! what is certain — a permissive reading over several dialects must never cry wolf:
//!
//! - **an unknown table**: on SQLite and PostgreSQL only, a name read where a table goes
//!   that no schema has, nor the other relations a query reads (partitions, sequences);
//!   never one the text creates (`CREATE [TEMP] TABLE`, `CREATE VIEW`, `SELECT … INTO`,
//!   anywhere in it: a console's statements run in any order) or the caller knows (made by
//!   the console's session), never a CTE, never one of a schema that wasn't read (`temp.`,
//!   an attached database, `information_schema`), never the engine's own (`sqlite_…`,
//!   `pg_…`), never in a schema change's own names (`DROP TABLE IF EXISTS t`);
//! - **an unknown column**: one written with a qualifier resolved surely to a database table
//!   whose columns were read (`u.nope`), or one of a statement's target list (`INSERT INTO t
//!   (nope)`, `UPDATE t SET nope =`), the engine's own columns aside (`rowid`, `ctid`…);
//! - **an ambiguous column** (a warning): an unqualified name surely an operand that two
//!   tables of the nearest level having it both have — unless they're joined by `USING` /
//!   `NATURAL` or it's one of the select list's aliases.
//!
//! What isn't known (columns not read yet) is left alone and said
//! ([`SchemaCheck::missing`]): asked for, the text checked again when they come. Grammar
//! errors are never reported here; the server's own come after a run.

use std::ops::Range;

use crate::TableRef;
use crate::navigation::{called, columns_of};
use crate::schema_index::SchemaIndex;
use crate::sql::lex::Token;
use crate::sql::resolve::{NameRole, Resolution, resolve};
use crate::sql::scope::TableSource;
use crate::sql::{Clause, Flavor, SplitOptions, created_tables, split};

/// What's wrong with a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No table or view is called `name` (as written, its schema included).
    UnknownTable { name: String },
    /// Table `table` has no column `column`.
    UnknownColumn { column: String, table: String },
    /// Tables `tables` (as the statement calls them) all have column `column`.
    Ambiguous { column: String, tables: Vec<String> },
}

impl Problem {
    /// It's a warning, not an error.
    pub fn is_warning(&self) -> bool {
        matches!(self, Self::Ambiguous { .. })
    }
}

/// A problem and where it's written (bytes of the text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaProblem {
    pub range: Range<usize>,
    pub problem: Problem,
}

/// What a check found, and what it needs to know more.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaCheck {
    /// In the text's order.
    pub problems: Vec<SchemaProblem>,
    /// Tables whose columns weren't read yet (the index's second tier): to ask for.
    pub missing: Vec<TableRef>,
}

/// What a check is asked with.
#[derive(Debug, Clone, Copy)]
pub struct CheckRequest<'a> {
    pub text: &'a str,
    /// The whole text's tokens.
    pub tokens: &'a [Token],
    pub flavor: Flavor,
    pub index: &'a SchemaIndex,
    /// Tables known besides the schema's (made by the console's session: its temporary
    /// ones).
    pub known: &'a [TableRef],
    /// The session attached other databases (SQLite): an unqualified name may be theirs.
    pub attached: bool,
}

/// The engine's own columns, which no listing has.
const SYSTEM_COLUMNS: &[&str] =
    &["rowid", "_rowid_", "oid", "ctid", "xmin", "xmax", "cmin", "cmax", "tableoid"];

/// The text checked (see the module's doc).
pub fn check(request: &CheckRequest<'_>) -> SchemaCheck {
    let CheckRequest { text, tokens, flavor, index, known, attached } = *request;
    let mut created = created_tables(text, tokens, flavor);
    created.extend(known.iter().cloned());
    let attached =
        attached || (flavor == Flavor::Sqlite && tokens.iter().any(|t| t.is_word(text, "ATTACH")));
    let mut out = SchemaCheck::default();
    for statement in split(text, tokens, flavor, &SplitOptions::default()) {
        if statement.incomplete {
            continue;
        }
        let res = resolve(text, &tokens[statement.tokens.clone()], flavor);
        let checker = Checker { res: &res, index, flavor, created: &created, attached };
        checker.run(&mut out);
    }
    out.missing.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
    out.missing.dedup();
    out
}

struct Checker<'a> {
    res: &'a Resolution,
    index: &'a SchemaIndex,
    flavor: Flavor,
    created: &'a [TableRef],
    attached: bool,
}

impl Checker<'_> {
    fn run(&self, out: &mut SchemaCheck) {
        for name in &self.res.names {
            if name.ddl {
                continue;
            }
            let Some(scope) = self.res.scopes.get(name.level) else { continue };
            let problem = match &name.role {
                NameRole::Table { table } => {
                    scope.tables.get(*table).and_then(|t| match &t.source {
                        TableSource::Table(r) => self.unknown_table(r),
                        _ => None,
                    })
                }
                NameRole::Column { qualifier, .. } if !qualifier.is_empty() => {
                    match self.res.qualified(name.level, qualifier)[..] {
                        [table] => self.unknown_column(scope, table, &name.text, out),
                        _ => None,
                    }
                }
                NameRole::TargetColumn { table } => {
                    self.unknown_column(scope, *table, &name.text, out)
                }
                // An `ORDER BY`'s name may be an output column's (`SELECT u.id … ORDER BY id`).
                NameRole::Column { certain: true, .. } if name.clause != Clause::OrderBy => {
                    self.ambiguous(scope, &name.text, out)
                }
                _ => None,
            };
            if let Some(problem) = problem {
                out.problems.push(SchemaProblem { range: name.range.clone(), problem });
            }
        }
    }

    fn unknown_table(&self, table: &TableRef) -> Option<Problem> {
        if !matches!(self.flavor, Flavor::Sqlite | Flavor::Postgres { .. }) {
            return None;
        }
        let lower = table.name.to_ascii_lowercase();
        let own = match self.flavor {
            Flavor::Sqlite => lower.starts_with("sqlite_"),
            _ => lower.starts_with("pg_"),
        };
        if own || self.created.iter().any(|c| c.name.eq_ignore_ascii_case(&table.name)) {
            return None;
        }
        match &table.schema {
            Some(schema) if !self.index.knows_schema(schema) => return None,
            None if self.attached => return None,
            _ => {}
        }
        (!self.index.exists(table)).then(|| Problem::UnknownTable { name: table.to_string() })
    }

    fn unknown_column(
        &self,
        scope: &crate::sql::LevelScope,
        table: usize,
        column: &str,
        out: &mut SchemaCheck,
    ) -> Option<Problem> {
        let t = scope.tables.get(table)?;
        let TableSource::Table(r) = &t.source else { return None };
        let i = self.index.find(r)?;
        if self.index.table(i).columns.is_none() {
            out.missing.push(self.index.table(i).table.clone());
            return None;
        }
        if self.index.column(i, column).is_some()
            || SYSTEM_COLUMNS.iter().any(|s| s.eq_ignore_ascii_case(column))
        {
            return None;
        }
        Some(Problem::UnknownColumn {
            column: column.to_owned(),
            table: self.index.table(i).table.name.clone(),
        })
    }

    fn ambiguous(
        &self,
        scope: &crate::sql::LevelScope,
        column: &str,
        out: &mut SchemaCheck,
    ) -> Option<Problem> {
        if scope.using || scope.aliases.iter().any(|a| a.eq_ignore_ascii_case(column)) {
            return None;
        }
        let deepest = scope.tables.iter().map(|t| t.level).max().unwrap_or(0);
        for depth in 0..=deepest {
            let mut owners = Vec::new();
            let mut unknown = false;
            for t in scope.tables.iter().filter(|t| t.level == depth) {
                match columns_of(t, Some(self.index), &mut out.missing) {
                    Some(names) if names.iter().any(|n| n.eq_ignore_ascii_case(column)) => {
                        owners.push(called(t));
                    }
                    Some(_) => {}
                    None => unknown = true,
                }
            }
            if owners.len() >= 2 {
                return Some(Problem::Ambiguous { column: column.to_owned(), tables: owners });
            }
            if !owners.is_empty() || unknown {
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/schema_check.rs"]
mod tests;
