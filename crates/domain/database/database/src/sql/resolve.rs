//! Every name of a statement with what it is, for going to a definition, the hover card and
//! the schema's diagnostics: a table named where one goes (and the schema before it), an
//! alias given, a CTE defined, a column (with the qualifier written before it), a
//! qualifier (`u` in `u.name`), a function called, a column of one table (`INSERT INTO t
//! (a)`, `UPDATE t SET a =`), each with the scope it's read in (the tables visible there:
//! the level's own, then the outer ones its kind sees; the CTEs; the select list's aliases).
//!
//! Read in one pass by the same scanner as [`super::scope`] (so both agree on what's in
//! scope), on the statement's tokens; pure.

use std::ops::Range;

use super::Flavor;
use super::lex::Token;
use super::scope::{Clause, Cte, ScopeTable};

/// What a name is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRole {
    /// A table (or view, CTE, function's rows) named where one goes: `table` is its index
    /// in its scope's tables.
    Table { table: usize },
    /// The schema (or database) written before table `table`'s name.
    Schema { table: usize },
    /// Where table `table`'s alias is given (`FROM users u`).
    Alias { table: usize },
    /// Where a CTE is defined (`WITH recent AS (…)`).
    CteName,
    /// A name in an expression: a column (or a select list's alias), `qualifier` written
    /// before it (`u.name`: `[u]`). `certain`: unqualified, it's surely an operand (not a
    /// type, an alias given, a part of a call's syntax): what an ambiguity is told on.
    Column { qualifier: Vec<String>, certain: bool },
    /// A qualifier: this name and those before it (`u` in `u.name`: `[u]`).
    Qualifier { parts: Vec<String> },
    /// A function called (`count(`), with its schema if written.
    Function { qualifier: Vec<String> },
    /// A column of table `table` only: `INSERT INTO t (|a)`, `UPDATE t SET |a =`,
    /// `ON CONFLICT (|a)`, `CREATE INDEX … ON t (|a)`.
    TargetColumn { table: usize },
}

/// A name of the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    /// Where it's written (its quotes included).
    pub range: Range<usize>,
    /// As it names its object (unquoted).
    pub text: String,
    pub quoted: bool,
    /// Its scope (an index in [`Resolution::scopes`]).
    pub level: usize,
    pub role: NameRole,
    /// The clause it's read in (an expression's; a table's `From`).
    pub clause: Clause,
    /// Its query is a schema change's own (`DROP TABLE t`, `REFERENCES t`).
    pub ddl: bool,
}

/// A query level's scope.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelScope {
    /// The tables visible there: its own first (their [`ScopeTable::level`] 0), then the
    /// outer ones it sees.
    pub tables: Vec<ScopeTable>,
    /// How many of them are its own.
    pub own: usize,
    pub ctes: Vec<Cte>,
    /// Its select list's aliases.
    pub aliases: Vec<String>,
    /// It joins by `USING (…)` or `NATURAL` (the columns both sides have are merged).
    pub using: bool,
}

/// A statement's names and their scopes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolution {
    /// In the text's order.
    pub names: Vec<Name>,
    pub scopes: Vec<LevelScope>,
}

impl Resolution {
    /// The name written at `offset` (in it, or right after it).
    pub fn name_at(&self, offset: usize) -> Option<&Name> {
        self.names.iter().find(|n| n.range.start <= offset && offset <= n.range.end)
    }

    /// The tables `qualifier` names in `level`'s scope (the innermost answering: an alias
    /// hides an outer table of the same name), as indexes in its tables.
    pub fn qualified(&self, level: usize, qualifier: &[String]) -> Vec<usize> {
        let Some(scope) = self.scopes.get(level) else { return Vec::new() };
        let mut found: Vec<usize> =
            (0..scope.tables.len()).filter(|&i| scope.tables[i].answers_to(qualifier)).collect();
        if let Some(nearest) = found.iter().map(|&i| scope.tables[i].level).min() {
            found.retain(|&i| scope.tables[i].level == nearest);
        }
        found
    }

    /// Where table `table` of scope `level` has its alias given.
    pub fn alias_of(&self, level: usize, table: usize) -> Option<&Name> {
        let t = self.scopes.get(level)?.tables.get(table)?;
        t.alias.as_ref()?;
        self.names.iter().find(|n| match n.role {
            NameRole::Alias { table: a } => self
                .scopes
                .get(n.level)
                .and_then(|s| s.tables.get(a))
                .is_some_and(|x| x.range == t.range),
            _ => false,
        })
    }
}

/// The names of one statement (`statement`: its tokens, of `text`), by `flavor`'s rules.
pub fn resolve(text: &str, statement: &[Token], flavor: Flavor) -> Resolution {
    super::scope::names(text, statement, flavor)
}

/// The names of the statement at `offset` of `text` (`tokens`: the whole text's), and
/// where that statement is (bytes).
pub fn resolve_at(
    text: &str,
    tokens: &[Token],
    offset: usize,
    flavor: Flavor,
) -> (Resolution, Range<usize>) {
    let statements = super::split(text, tokens, flavor, &super::SplitOptions::default());
    let found = statements.iter().find(|s| {
        s.range.start <= offset
            && (offset <= s.range.end || s.terminator.as_ref().is_some_and(|t| offset <= t.start))
    });
    match found {
        Some(s) => (resolve(text, &tokens[s.tokens.clone()], flavor), s.range.clone()),
        None => (Resolution::default(), offset..offset),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/resolve.rs"]
mod tests;
