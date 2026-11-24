//! The columns offered: after `alias.`, unqualified across the tables in scope, an
//! `INSERT`'s list, `USING`, and `SELECT *` expanded.

use std::ops::Range;

use crate::TableRef;
use crate::sql::Clause;
use crate::sql::scope::{ScopeTable, TableSource};

use super::keywords::EXPRESSION_WORDS;
use super::{Completer, Ranked, Suggestion, SuggestionKind, column_detail};

impl<'a> Completer<'a> {
    /// `alias.|`, `table.|`, `schema.table.|`, `schema.|`.
    pub(super) fn qualified(&mut self, qualifier: &[String]) {
        let found = self.scope.qualified(qualifier);
        if !found.is_empty() {
            for t in found {
                let table = self.scope.tables[t].clone();
                let owner = Self::owner(&table);
                if let Some(columns) = self.scope_columns(&table) {
                    for (name, type_name) in columns {
                        let item = Suggestion::new(name.clone(), SuggestionKind::Column)
                            .insert(self.name(&name))
                            .detail(column_detail(&owner, type_name.as_deref()));
                        self.push_by(0, &name, item);
                    }
                }
            }
            return;
        }
        // A table not in scope (`schema.table.|`, `table.|`): its columns.
        let Some(index) = self.index else { return };
        let r = match qualifier {
            [.., s, n] => TableRef::in_schema(s.clone(), n.clone()),
            [n] => TableRef::new(n.clone()),
            [] => return,
        };
        if let Some(i) = index.find(&r)
            && (qualifier.len() > 1
                || !index.schemas().iter().any(|s| s.eq_ignore_ascii_case(&r.name)))
        {
            let fake = ScopeTable {
                source: TableSource::Table(index.table(i).table.clone()),
                alias: None,
                level: 0,
                range: 0..0,
            };
            let owner = index.table(i).table.name.clone();
            if let Some(columns) = self.scope_columns(&fake) {
                for (name, type_name) in columns {
                    let item = Suggestion::new(name.clone(), SuggestionKind::Column)
                        .insert(self.name(&name))
                        .detail(column_detail(&owner, type_name.as_deref()));
                    self.push_by(0, &name, item);
                }
            }
            return;
        }
        // `schema.|`: its tables.
        if let [schema] = qualifier
            && index.schemas().iter().any(|s| s.eq_ignore_ascii_case(schema))
        {
            self.tables(Some(schema), false);
        }
    }

    /// An expression's suggestions: the columns in scope, the aliases, the functions and
    /// the words.
    pub(super) fn expression(&mut self) {
        self.expression_from(0);
    }

    pub(super) fn expression_from(&mut self, base: u8) {
        let alias_first = self.scope.clause == Clause::OrderBy;
        for alias in self.scope.aliases.clone() {
            let item =
                Suggestion::new(alias.clone(), SuggestionKind::Alias).insert(self.name(&alias));
            self.push(base + if alias_first { 0 } else { 2 }, item);
        }
        self.unqualified_columns(base + 1);
        // The tables' own names / aliases (to qualify with).
        for t in self.scope.tables.clone() {
            let name = t.name().to_owned();
            if name.is_empty() {
                continue;
            }
            let detail = match &t.source {
                TableSource::Table(r) if t.alias.is_some() => r.name.clone(),
                _ => String::new(),
            };
            let item = Suggestion::new(name.clone(), SuggestionKind::Alias)
                .insert(self.name(&name))
                .detail(detail);
            self.push(base + 3, item);
        }
        if !self.query.is_empty() {
            self.functions(base + 4);
        }
        self.keywords_list(EXPRESSION_WORDS, base + 5);
    }

    /// The columns of every table in scope, unqualified: the cursor's query's first; one
    /// that two tables of the same level have is inserted qualified (`o.id`).
    fn unqualified_columns(&mut self, group: u8) {
        let tables = self.scope.tables.clone();
        let mut all: Vec<(usize, String, Option<String>)> = Vec::new();
        for (k, t) in tables.iter().enumerate() {
            if let Some(columns) = self.scope_columns(t) {
                for (name, type_name) in columns {
                    all.push((k, name, type_name));
                }
            }
        }
        for (k, name, type_name) in &all {
            let t = &tables[*k];
            let ambiguous = all.iter().any(|(o, n, _)| {
                o != k && n.eq_ignore_ascii_case(name) && tables[*o].level == t.level
            });
            let insert = if ambiguous && !t.name().is_empty() {
                format!("{}.{}", self.name(t.name()), self.name(name))
            } else {
                self.name(name)
            };
            let owner = Self::owner(t);
            let label = if ambiguous { format!("{}.{}", t.name(), name) } else { name.clone() };
            let item = Suggestion::new(label, SuggestionKind::Column)
                .insert(insert)
                .detail(column_detail(&owner, type_name.as_deref()));
            self.push_by(group, name, item);
        }
    }

    /// One table's columns (`UPDATE t SET |`, an `INSERT`'s list).
    pub(super) fn columns_of(&mut self, table: usize, group: u8, qualified: bool) {
        let Some(t) = self.scope.tables.get(table).cloned() else { return };
        let owner = Self::owner(&t);
        if let Some(columns) = self.scope_columns(&t) {
            for (name, type_name) in columns {
                let insert = if qualified {
                    format!("{}.{}", self.name(t.name()), self.name(&name))
                } else {
                    self.name(&name)
                };
                let item = Suggestion::new(name.clone(), SuggestionKind::Column)
                    .insert(insert)
                    .detail(column_detail(&owner, type_name.as_deref()));
                self.push_by(group, &name, item);
            }
        }
    }

    /// `INSERT INTO t (|`: all its columns at once, first.
    pub(super) fn all_columns(&mut self, table: usize) {
        let Some(t) = self.scope.tables.get(table).cloned() else { return };
        let Some(columns) = self.scope_columns(&t) else { return };
        if columns.is_empty() {
            return;
        }
        let list = columns.iter().map(|(n, _)| self.name(n)).collect::<Vec<_>>().join(", ");
        let item = Suggestion::new(list.clone(), SuggestionKind::Columns).insert(list);
        // Always shown first, whatever is typed.
        let order = self.items.len();
        self.items.push(Ranked { group: 0, score: i32::MAX, order, item });
    }

    /// `JOIN t USING (|`: the columns `t` shares with a table before it.
    pub(super) fn using(&mut self, right: usize, left: &[usize]) {
        let Some(rt) = self.scope.tables.get(right).cloned() else { return };
        let Some(own) = self.scope_columns(&rt) else { return };
        let mut others: Vec<String> = Vec::new();
        for &l in left {
            if let Some(t) = self.scope.tables.get(l).cloned()
                && let Some(c) = self.scope_columns(&t)
            {
                others.extend(c.into_iter().map(|(n, _)| n));
            }
        }
        for (name, type_name) in own {
            let shared = others.iter().any(|o| o.eq_ignore_ascii_case(&name));
            let item = Suggestion::new(name.clone(), SuggestionKind::Column)
                .insert(self.name(&name))
                .detail(column_detail(&Self::owner(&rt), type_name.as_deref()));
            self.push_by(if shared { 0 } else { 1 }, &name, item);
        }
    }

    /// `*` (or `t.*`) expanded: the columns it means, qualified when several tables are.
    pub(super) fn expand(&mut self, range: Range<usize>, qualifier: Option<String>) {
        let tables: Vec<ScopeTable> = match &qualifier {
            Some(q) => self
                .scope
                .qualified(std::slice::from_ref(q))
                .into_iter()
                .map(|i| self.scope.tables[i].clone())
                .collect(),
            None => self.scope.tables.iter().filter(|t| t.level == 0).cloned().collect(),
        };
        if tables.is_empty() {
            return;
        }
        let qualify = qualifier.is_some() || tables.len() > 1;
        let mut names: Vec<String> = Vec::new();
        for t in &tables {
            let Some(columns) = self.scope_columns(t) else { return };
            for (name, _) in columns {
                if qualify && !t.name().is_empty() {
                    names.push(format!("{}.{}", self.name(t.name()), self.name(&name)));
                } else {
                    names.push(self.name(&name));
                }
            }
        }
        if names.is_empty() {
            return;
        }
        let list = names.join(", ");
        let mut item = Suggestion::new(list.clone(), SuggestionKind::Columns).insert(list);
        item.expansion = true;
        self.replace = Some(range);
        let order = self.items.len();
        self.items.push(Ranked { group: 0, score: i32::MAX, order, item });
    }
}
