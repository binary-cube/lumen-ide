//! What the console suggests at the cursor: from what's in scope there
//! ([`crate::sql::scope`]) and the source's schema ([`SchemaIndex`]) — the keywords fitting
//! the place, the tables and views (the current schema's first, then the others qualified,
//! with their schema in the detail), the columns (after `alias.` that table's; unqualified
//! every table's in scope, the ambiguous ones inserted qualified), an `INSERT`'s column
//! list (all of them first), `SELECT *` expanded, whole `JOIN … ON` conditions from the
//! foreign keys and the accepted virtual keys (those found from names last, marked), the
//! tables related to those in scope first after `JOIN`, the CTEs and derived tables with
//! their columns, the select list's aliases where they're allowed, the functions with their
//! signatures, the literals of an enum column where its value is written (`status = |`,
//! `IN (|`, an `INSERT`'s values, an open `'…`).
//!
//! Pure and cheap: a prefix is a binary search over the index's sorted names, a fuzzy match
//! looks at a few hundred at most (well under 2 ms at 5 000 tables). Columns not read yet
//! (a huge schema's second tier) are said ([`Completion::missing`]): the caller asks for
//! them in the background and asks again when they come.

use std::ops::Range;

use crate::schema_index::{IndexRelation, RelationOrigin, SchemaIndex, score};
use crate::sql::scope::{Expect, KeywordPlace, OutputColumn, QueryScope, ScopeTable, TableSource};
use crate::sql::value::ValueOf;
use crate::sql::{Clause, Flavor};
use crate::{RoutineKind, TableKind, TableRef};
use std::sync::Arc;

/// What a suggestion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestionKind {
    Keyword,
    Schema,
    Table,
    View,
    /// One of the statement's CTEs.
    Cte,
    Column,
    /// A table's alias in scope (or a select list's alias).
    Alias,
    Function,
    /// A table joined with its condition, or a whole condition.
    Join,
    /// Every column (an `INSERT`'s list, `*` expanded).
    Columns,
    /// A value the column allows (an enum's literal).
    Value,
    /// The columns are being read: the list is made again when they come.
    Loading,
}

/// A suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    /// What replaces the word typed.
    pub insert: String,
    /// Shown on the right: a table's schema, a column's table and type, a function's
    /// signature.
    pub detail: Option<String>,
    pub kind: SuggestionKind,
    /// How many characters back the cursor goes after it's inserted (into `count()`).
    pub cursor_back: usize,
    /// Found from names, not decided by the user: shown last, marked.
    pub inferred: bool,
    /// Among the tables: of the current schema.
    pub current: bool,
    /// `*` expanded (Expand Column List).
    pub expansion: bool,
}

impl Suggestion {
    fn new(label: impl Into<String>, kind: SuggestionKind) -> Self {
        let label = label.into();
        Self {
            insert: label.clone(),
            label,
            detail: None,
            kind,
            cursor_back: 0,
            inferred: false,
            current: false,
            expansion: false,
        }
    }

    fn insert(mut self, insert: impl Into<String>) -> Self {
        self.insert = insert.into();
        self
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }
}

/// What's suggested, and what's missing for more.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    /// Best first.
    pub items: Vec<Suggestion>,
    /// What the suggestions replace when it isn't the word typed (`*` expanded).
    pub replace: Option<Range<usize>>,
    /// Tables whose columns weren't read yet (the index's second tier): to ask for.
    pub missing: Vec<TableRef>,
}

/// What a completion is asked with.
#[derive(Debug, Clone, Copy)]
pub struct CompletionRequest<'a> {
    pub scope: &'a QueryScope,
    /// The source's schema (`None`: not read yet: keywords, the statement's own names).
    pub index: Option<&'a SchemaIndex>,
    pub flavor: Flavor,
    /// The schema the console runs in (its own choice, else the connection's).
    pub schema: Option<&'a str>,
    /// The most suggestions given.
    pub limit: usize,
}

/// How many suggestions a list holds at most by default.
pub const LIMIT: usize = 200;

/// The suggestions at the cursor (see the module's doc).
pub fn complete(request: &CompletionRequest<'_>) -> Completion {
    let mut c = Completer::new(request);
    c.run();
    c.finish()
}

/// An enum column found: its table's name, its name, its values.
type EnumColumn = (String, String, Arc<[Box<str>]>);

/// A suggestion ranked: its group (lower first), its score, the order it came in.
struct Ranked {
    group: u8,
    score: i32,
    order: usize,
    item: Suggestion,
}

struct Completer<'a> {
    req: &'a CompletionRequest<'a>,
    scope: &'a QueryScope,
    index: Option<&'a SchemaIndex>,
    query: String,
    items: Vec<Ranked>,
    missing: Vec<TableRef>,
    replace: Option<Range<usize>>,
    /// Keywords written in lower case (the word typed is).
    lower: bool,
}

impl<'a> Completer<'a> {
    fn new(req: &'a CompletionRequest<'a>) -> Self {
        let prefix = &req.scope.prefix;
        Self {
            req,
            scope: req.scope,
            index: req.index,
            query: prefix.to_lowercase(),
            items: Vec::new(),
            missing: Vec::new(),
            replace: None,
            lower: !prefix.is_empty() && prefix.chars().all(|c| !c.is_uppercase()),
        }
    }

    /// `item` in `group`, when it matches what's typed (by `key`, its label by default).
    fn push(&mut self, group: u8, item: Suggestion) {
        let key = item.label.clone();
        self.push_by(group, &key, item);
    }

    fn push_by(&mut self, group: u8, key: &str, item: Suggestion) {
        let Some(score) = score(&self.query, key) else { return };
        let order = self.items.len();
        self.items.push(Ranked { group, score, order, item });
    }

    fn finish(mut self) -> Completion {
        self.items.sort_by(|a, b| {
            a.group.cmp(&b.group).then(b.score.cmp(&a.score)).then(a.order.cmp(&b.order))
        });
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut items = Vec::with_capacity(self.items.len().min(self.req.limit));
        let typed = self.typed_raw();
        for r in std::mem::take(&mut self.items) {
            let key = (r.item.label.clone(), r.item.insert.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            items.push(r.item);
            if items.len() >= self.req.limit {
                break;
            }
        }
        self.missing.dedup();
        // In a literal nothing stands for the word typed (its quote is in the range): the
        // columns are asked for all the same.
        if !self.missing.is_empty()
            && self.scope.expect != Expect::Literal
            && !items.iter().any(|i| i.kind == SuggestionKind::Loading)
        {
            let mut loading = Suggestion::new("", SuggestionKind::Loading);
            loading.insert = typed;
            items.push(loading);
        }
        Completion { items, replace: self.replace, missing: self.missing }
    }

    /// The word typed as written (its quote included).
    fn typed_raw(&self) -> String {
        if self.scope.quoted {
            format!("\"{}", self.scope.prefix)
        } else {
            self.scope.prefix.clone()
        }
    }

    fn run(&mut self) {
        if let Some(star) = &self.scope.star {
            self.expand(star.range.clone(), star.qualifier.clone());
            if !self.items.is_empty() {
                return;
            }
        }
        // Where a column's value goes: what its type allows, first.
        self.literals();
        match self.scope.expect.clone() {
            Expect::Nothing | Expect::Literal => {}
            Expect::Keyword(place) => self.keywords(place, 0),
            Expect::Schema => self.schemas(0),
            Expect::Table { schema, join } => self.tables(schema.as_deref(), join),
            Expect::Column { qualifier: Some(q) } => self.qualified(&q),
            Expect::Column { qualifier: None } => self.expression(),
            Expect::JoinCondition { right, left } => {
                self.conditions(right, &left);
                self.expression_from(5);
            }
            Expect::InsertColumns { table } => {
                self.all_columns(table);
                self.columns_of(table, 1, false);
            }
            Expect::TableColumns { table } => self.columns_of(table, 0, false),
            Expect::UsingColumns { right, left } => self.using(right, &left),
            Expect::Value => {
                self.keywords_list(VALUE_WORDS, 1);
                if !self.query.is_empty() {
                    self.functions(2);
                }
            }
        }
    }

    // ─── Values ───

    /// The literals of the enum column whose value is written at the cursor, in their
    /// declared order, quoted (inside an open literal, it's replaced whole).
    fn literals(&mut self) {
        let Some(value_of) = self.scope.value_of.clone() else { return };
        let Some((owner, column, values)) = self.value_column(&value_of) else { return };
        let detail = format!("{owner}.{column}");
        for value in values.iter() {
            let item = Suggestion::new(value.to_string(), SuggestionKind::Value)
                .insert(crate::sql::value::literal(value))
                .detail(detail.clone());
            self.push_by(0, &value.to_lowercase(), item);
        }
    }

    /// The column `value_of` names among the tables in scope (the innermost first): its
    /// table's name, its name, its values (`None`: not an enum, or not known — its columns
    /// asked for).
    fn value_column(&mut self, value_of: &ValueOf) -> Option<EnumColumn> {
        let index = self.index?;
        let (tables, wanted): (Vec<usize>, Result<&str, usize>) = match value_of {
            ValueOf::Column { qualifier, name } if qualifier.is_empty() => {
                let mut all: Vec<usize> = (0..self.scope.tables.len()).collect();
                all.sort_by_key(|&i| self.scope.tables[i].level);
                (all, Ok(name.as_str()))
            }
            ValueOf::Column { qualifier, name } => {
                (self.scope.qualified(qualifier), Ok(name.as_str()))
            }
            ValueOf::Position { table, position } => (self.scope.qualified(table), Err(*position)),
        };
        for t in tables {
            let scope_table = self.scope.tables[t].clone();
            let Some(i) = self.resolve(&scope_table) else { continue };
            let table = index.table(i);
            let Some(columns) = &table.columns else {
                self.missing.push(table.table.clone());
                continue;
            };
            let column = match wanted {
                Ok(name) => columns.iter().find(|c| c.name.eq_ignore_ascii_case(name)),
                Err(position) => columns.get(position),
            };
            if let Some(c) = column {
                let values = c.values.clone()?;
                return Some((table.table.name.clone(), c.name.to_string(), values));
            }
        }
        None
    }

    // ─── Names ───

    /// `name` as it's written into the statement (quoted when the engine would read it
    /// otherwise).
    fn name(&self, name: &str) -> String {
        quote(name, self.req.flavor)
    }

    /// Table `t` of the index as written from the console's schema: unqualified in it,
    /// else with its schema.
    fn table_text(&self, i: usize) -> String {
        let Some(index) = self.index else { return String::new() };
        let t = &index.table(i).table;
        if index.in_schema(i, self.req.schema) || t.schema.is_none() {
            self.name(&t.name)
        } else {
            format!("{}.{}", self.name(t.schema.as_deref().unwrap_or_default()), self.name(&t.name))
        }
    }

    /// The index's table a table in scope is.
    fn resolve(&self, t: &ScopeTable) -> Option<usize> {
        let index = self.index?;
        let table = t.table()?;
        index.find(table)
    }

    /// The columns of a table in scope (`None`: not known yet — asked for).
    fn scope_columns(&mut self, t: &ScopeTable) -> Option<Vec<(String, Option<String>)>> {
        match &t.source {
            TableSource::Table(r) => {
                let index = self.index?;
                let i = index.find(r)?;
                match &index.table(i).columns {
                    Some(c) => Some(
                        c.iter()
                            .map(|c| (c.name.to_string(), Some(c.type_name.to_string())))
                            .collect(),
                    ),
                    None => {
                        self.missing.push(index.table(i).table.clone());
                        None
                    }
                }
            }
            TableSource::Cte { columns, .. } | TableSource::Derived { columns } => {
                let mut out = Vec::new();
                for c in columns {
                    match c {
                        OutputColumn::Named(n) => out.push((n.clone(), None)),
                        OutputColumn::Star { tables, .. } => {
                            for r in tables {
                                let fake = ScopeTable {
                                    source: TableSource::Table(r.clone()),
                                    alias: None,
                                    level: 0,
                                    range: 0..0,
                                };
                                out.extend(self.scope_columns(&fake).unwrap_or_default());
                            }
                        }
                    }
                }
                Some(out)
            }
            TableSource::Function(_) => Some(Vec::new()),
        }
    }

    /// What a table in scope is called in a column's detail: `orders o`, `orders`.
    fn owner(t: &ScopeTable) -> String {
        let base = match &t.source {
            TableSource::Table(r) => r.name.clone(),
            TableSource::Cte { name, .. } | TableSource::Function(name) => name.clone(),
            TableSource::Derived { .. } => String::new(),
        };
        match &t.alias {
            Some(a) if !base.is_empty() && !a.eq_ignore_ascii_case(&base) => format!("{base} {a}"),
            Some(a) => a.clone(),
            None => base,
        }
    }

    // ─── Keywords ───

    fn keywords(&mut self, place: KeywordPlace, group: u8) {
        let words = keywords_for(place, self.req.flavor, self.scope.clause);
        self.keywords_list(words, group);
    }

    fn keywords_list(&mut self, words: &[&str], group: u8) {
        for word in words {
            if !fits(word, self.req.flavor) {
                continue;
            }
            let shown = if self.lower { word.to_lowercase() } else { (*word).to_owned() };
            let value = VALUE_WORDS.contains(word) || word.ends_with(')');
            let mut item = Suggestion::new(shown.clone(), SuggestionKind::Keyword);
            if word.ends_with("()") {
                item.cursor_back = 1;
                item.insert = shown;
            } else if !value {
                item.insert = format!("{shown} ");
            }
            let key = word.to_lowercase();
            self.push_by(group, &key, item);
        }
    }

    // ─── Schemas and tables ───

    fn schemas(&mut self, group: u8) {
        let Some(index) = self.index else { return };
        for s in index.schemas() {
            let item = Suggestion::new(s.clone(), SuggestionKind::Schema).insert(self.name(s));
            self.push(group, item);
        }
    }

    fn tables(&mut self, schema: Option<&str>, join: bool) {
        if schema.is_none() {
            // The statement's CTEs first.
            for cte in self.scope.ctes.clone() {
                let item = Suggestion::new(cte.name.clone(), SuggestionKind::Cte)
                    .insert(self.name(&cte.name));
                self.push(if join { 3 } else { 0 }, item);
            }
            if join {
                self.related();
            }
        }
        let Some(index) = self.index else { return };
        let limit = self.req.limit;
        let base = if join { 4 } else { 1 };
        for i in index.candidates(&self.query, limit * 4) {
            let t = index.table(i);
            let current = index.in_schema(i, self.req.schema);
            if let Some(schema) = schema
                && !index.schema_of(i).is_some_and(|s| s.eq_ignore_ascii_case(schema))
            {
                continue;
            }
            let explicit = schema.is_some();
            let label = if current || explicit || t.table.schema.is_none() {
                t.table.name.clone()
            } else {
                format!("{}.{}", t.table.schema.as_deref().unwrap_or_default(), t.table.name)
            };
            let insert = if explicit { self.name(&t.table.name) } else { self.table_text(i) };
            let kind = if t.kind == TableKind::View {
                SuggestionKind::View
            } else {
                SuggestionKind::Table
            };
            let mut item = Suggestion::new(label, kind).insert(insert);
            item.detail = index.schema_of(i).map(str::to_owned);
            item.current = current;
            let group = if current || explicit { base } else { base + 2 };
            let key = t.table.name.clone();
            self.push_by(group, &key, item);
        }
        if schema.is_none() {
            // The other schemas (to qualify a table with), after the current one's tables.
            for s in index.schemas().to_vec() {
                if self
                    .req
                    .schema
                    .or(index.default_schema())
                    .is_some_and(|c| c.eq_ignore_ascii_case(&s))
                {
                    continue;
                }
                let item = Suggestion::new(s.clone(), SuggestionKind::Schema)
                    .insert(format!("{}.", self.name(&s)));
                self.push(base + 1, item);
            }
        }
    }

    /// After `JOIN`: the tables related to those in scope, each with its alias and its
    /// whole condition (`orders o ON o.user_id = u.id`): real and accepted keys first,
    /// those found from names last.
    fn related(&mut self) {
        let Some(index) = self.index else { return };
        let own: Vec<ScopeTable> =
            self.scope.tables.iter().filter(|t| t.level == 0).cloned().collect();
        let mut taken: Vec<String> =
            self.scope.tables.iter().map(|t| t.name().to_lowercase()).collect();
        for left in &own {
            let Some(li) = self.resolve(left) else { continue };
            for r in index.relations_of(li).cloned().collect::<Vec<_>>() {
                let other = if r.from as usize == li { r.to } else { r.from } as usize;
                let alias = alias_for(&index.table(other).table.name, &taken);
                let right_name = alias.clone();
                let condition = self.condition_text(&r, other, &right_name, li, left.name());
                let text =
                    format!("{} {} ON {}", self.table_text(other), self.name(&alias), condition);
                let mut item = Suggestion::new(text.clone(), SuggestionKind::Join);
                item.detail = Some(relation_detail(index, &r));
                item.inferred = r.origin == RelationOrigin::Inferred;
                // The tables it points to first, then those pointing to it, then those
                // found from names.
                let group = if item.inferred {
                    2
                } else if r.from as usize == li {
                    0
                } else {
                    1
                };
                let key = index.table(other).table.name.clone();
                self.push_by(group, &key, item);
            }
            taken.push(left.name().to_lowercase());
        }
    }

    /// `relation`'s condition between table `right` (written `right_name`) and table
    /// `left` (written `left_name`), the right side first: `u.id = o.user_id`.
    fn condition_text(
        &self,
        r: &IndexRelation,
        right: usize,
        right_name: &str,
        left: usize,
        left_name: &str,
    ) -> String {
        let _ = left;
        // The table joined is the key's own (its columns point to the other's), or the one
        // pointed to (a tree's parent: `p.id = c.parent_id`).
        let right_is_from = r.from as usize == right && r.from != r.to;
        let (rc, lc) =
            if right_is_from { (&r.columns, &r.references) } else { (&r.references, &r.columns) };
        rc.iter()
            .zip(lc)
            .map(|(a, b)| {
                format!(
                    "{}.{} = {}.{}",
                    self.name(right_name),
                    self.name(a),
                    self.name(left_name),
                    self.name(b)
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// After `JOIN t ON`: the conditions between `t` and the tables before it.
    fn conditions(&mut self, right: usize, left: &[usize]) {
        let Some(index) = self.index else { return };
        let Some(rt) = self.scope.tables.get(right).cloned() else { return };
        let Some(ri) = self.resolve(&rt) else { return };
        for &l in left {
            let Some(lt) = self.scope.tables.get(l).cloned() else { continue };
            let Some(li) = self.resolve(&lt) else { continue };
            for r in index.relations_of(ri).cloned().collect::<Vec<_>>() {
                let other = if r.from as usize == ri { r.to } else { r.from } as usize;
                if other != li {
                    continue;
                }
                let text = self.condition_text(&r, ri, rt.name(), li, lt.name());
                let mut item = Suggestion::new(text, SuggestionKind::Join);
                item.detail = Some(relation_detail(index, &r));
                item.inferred = r.origin == RelationOrigin::Inferred;
                let group = if item.inferred { 2 } else { 0 };
                self.push_by(group, &self.query.clone(), item);
            }
        }
    }

    // ─── Columns ───

    /// `alias.|`, `table.|`, `schema.table.|`, `schema.|`.
    fn qualified(&mut self, qualifier: &[String]) {
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
    fn expression(&mut self) {
        self.expression_from(0);
    }

    fn expression_from(&mut self, base: u8) {
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
    fn columns_of(&mut self, table: usize, group: u8, qualified: bool) {
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
    fn all_columns(&mut self, table: usize) {
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
    fn using(&mut self, right: usize, left: &[usize]) {
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
    fn expand(&mut self, range: Range<usize>, qualifier: Option<String>) {
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

    // ─── Functions ───

    fn functions(&mut self, group: u8) {
        let flavor = self.req.flavor;
        let mut named: Vec<String> = Vec::new();
        if let Some(index) = self.index {
            for i in index.routine_candidates(&self.query, 120) {
                let r = &index.routines()[i];
                if r.kind == RoutineKind::Procedure
                    || named.iter().any(|n| n.eq_ignore_ascii_case(&r.name))
                {
                    continue;
                }
                named.push(r.name.clone());
                let signature = routine_signature(r, builtin_signature(&r.name, flavor));
                let shown =
                    if self.lower || !r.builtin { r.name.clone() } else { r.name.to_uppercase() };
                let mut item = Suggestion::new(shown.clone(), SuggestionKind::Function)
                    .insert(format!(
                        "{}()",
                        if r.builtin { shown.clone() } else { self.name(&r.name) }
                    ))
                    .detail(signature);
                item.cursor_back = 1;
                let key = r.name.to_lowercase();
                self.push_by(group, &key, item);
            }
        }
        for (name, signature, f) in BUILTINS {
            if !fits_flavor(*f, flavor) || named.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            let shown = if self.lower { name.to_lowercase() } else { name.to_uppercase() };
            let mut item = Suggestion::new(shown.clone(), SuggestionKind::Function)
                .insert(format!("{shown}()"))
                .detail(*signature);
            item.cursor_back = 1;
            self.push_by(group, &name.to_lowercase(), item);
        }
    }
}

/// A column's detail: its table, its type.
fn column_detail(owner: &str, type_name: Option<&str>) -> String {
    match type_name.filter(|t| !t.is_empty()) {
        Some(t) if !owner.is_empty() => format!("{owner} · {t}"),
        Some(t) => t.to_owned(),
        None => owner.to_owned(),
    }
}

/// A relation's detail: `orders.user_id → users.id`.
fn relation_detail(index: &SchemaIndex, r: &IndexRelation) -> String {
    let side = |t: u32, columns: &[String]| {
        let name = &index.table(t as usize).table.name;
        match columns {
            [one] => format!("{name}.{one}"),
            many => format!("{name}.({})", many.join(", ")),
        }
    };
    format!("{} → {}", side(r.from, &r.columns), side(r.to, &r.references))
}

/// An alias for table `name` none of `taken` is: its words' initials (`order_items` →
/// `oi`, `OrderItems` → `oi`, `users` → `u`), a number after it when taken.
pub fn alias_for(name: &str, taken: &[String]) -> String {
    let mut initials = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        let start = match prev {
            None => true,
            Some(p) => matches!(p, '_' | '-' | ' ' | '.') || (p.is_lowercase() && c.is_uppercase()),
        };
        if start && c.is_alphanumeric() {
            initials.extend(c.to_lowercase());
        }
        prev = Some(c);
    }
    if initials.is_empty() {
        initials.push('t');
    }
    let is_taken = |a: &str| {
        taken.iter().any(|t| t.eq_ignore_ascii_case(a))
            || RESERVED.iter().any(|r| r.eq_ignore_ascii_case(a))
    };
    if !is_taken(&initials) {
        return initials;
    }
    (1..).map(|n| format!("{initials}{n}")).find(|a| !is_taken(a)).unwrap_or(initials)
}

/// `name` quoted when `flavor`'s engine would otherwise read it differently: not a plain
/// identifier, a reserved word, or (PostgreSQL) with capitals, which it folds.
pub fn quote(name: &str, flavor: Flavor) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && name.chars().all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        && !(flavor.is_postgres() && name.chars().any(|c| c.is_ascii_uppercase()))
        && !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name));
    if plain {
        name.to_owned()
    } else {
        let quote = if flavor == Flavor::MySql { '`' } else { '"' };
        let doubled: String = [quote, quote].iter().collect();
        format!("{quote}{}{quote}", name.replace(quote, &doubled))
    }
}

/// Words a name can't be written as (the engines' reserved ones, common to them).
const RESERVED: &[&str] = &[
    "all",
    "and",
    "any",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "do",
    "drop",
    "else",
    "end",
    "except",
    "exists",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "if",
    "in",
    "index",
    "inner",
    "insert",
    "intersect",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "natural",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "select",
    "set",
    "table",
    "then",
    "to",
    "true",
    "union",
    "unique",
    "update",
    "user",
    "using",
    "values",
    "when",
    "where",
    "with",
];

/// Which engines a word or a function is theirs: all, SQLite's, PostgreSQL's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Of {
    All,
    Sqlite,
    Postgres,
    /// Not SQLite's.
    NotSqlite,
    /// MySQL's / MariaDB's.
    MySql,
}

fn fits_flavor(of: Of, flavor: Flavor) -> bool {
    match of {
        Of::All => true,
        Of::Sqlite => matches!(flavor, Flavor::Sqlite | Flavor::Generic),
        Of::Postgres => matches!(flavor, Flavor::Postgres { .. } | Flavor::Generic),
        Of::NotSqlite => flavor != Flavor::Sqlite,
        Of::MySql => matches!(flavor, Flavor::MySql | Flavor::Generic),
    }
}

/// Whether a keyword is `flavor`'s.
fn fits(word: &str, flavor: Flavor) -> bool {
    const PG_ONLY: &[&str] = &[
        "ILIKE",
        "FULL JOIN",
        "LATERAL",
        "DISTINCT ON",
        "FETCH FIRST",
        "FOR UPDATE",
        "FOR SHARE",
        "TRUNCATE",
        "SHOW",
        "MERGE INTO",
        "COPY",
        "SIMILAR TO",
        "NULLS FIRST",
        "NULLS LAST",
        "ANALYZE",
    ];
    const SQLITE_ONLY: &[&str] = &[
        "PRAGMA",
        "GLOB",
        "REPLACE INTO",
        "INSERT OR REPLACE INTO",
        "INSERT OR IGNORE INTO",
        "ATTACH DATABASE",
        "INDEXED BY",
    ];
    match flavor {
        Flavor::Sqlite => {
            !PG_ONLY.contains(&word)
                || matches!(word, "NULLS FIRST" | "NULLS LAST" | "ANALYZE" | "FULL JOIN")
        }
        Flavor::Postgres { .. } => !SQLITE_ONLY.contains(&word),
        _ => true,
    }
}

/// The words fitting `place`.
fn keywords_for(place: KeywordPlace, flavor: Flavor, clause: Clause) -> &'static [&'static str] {
    let _ = flavor;
    match place {
        KeywordPlace::Start => &[
            "SELECT",
            "WITH",
            "INSERT INTO",
            "UPDATE",
            "DELETE FROM",
            "VALUES",
            "CREATE TABLE",
            "CREATE INDEX",
            "CREATE VIEW",
            "ALTER TABLE",
            "DROP TABLE",
            "TRUNCATE",
            "EXPLAIN",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "MERGE INTO",
            "REPLACE INTO",
            "INSERT OR REPLACE INTO",
            "PRAGMA",
            "SHOW",
            "SET",
            "ANALYZE",
            "VACUUM",
            "ATTACH DATABASE",
            "COPY",
        ],
        KeywordPlace::AfterCte => &["SELECT", "INSERT INTO", "UPDATE", "DELETE FROM", "MERGE INTO"],
        KeywordPlace::AfterSelectItem => &[
            "FROM",
            "AS",
            "AND",
            "OR",
            "IS NULL",
            "IS NOT NULL",
            "IN",
            "LIKE",
            "BETWEEN",
            "OVER",
            "FILTER",
            "INTO",
            "UNION",
            "UNION ALL",
            "ORDER BY",
            "LIMIT",
            "COLLATE",
        ],
        KeywordPlace::AfterTable { join: true } => &[
            "ON",
            "USING",
            "AS",
            "WHERE",
            "JOIN",
            "INNER JOIN",
            "LEFT JOIN",
            "RIGHT JOIN",
            "FULL JOIN",
            "CROSS JOIN",
            "GROUP BY",
            "ORDER BY",
            "LIMIT",
        ],
        KeywordPlace::AfterTable { join: false } => &[
            "WHERE",
            "JOIN",
            "INNER JOIN",
            "LEFT JOIN",
            "RIGHT JOIN",
            "FULL JOIN",
            "CROSS JOIN",
            "NATURAL JOIN",
            "LEFT OUTER JOIN",
            "GROUP BY",
            "HAVING",
            "ORDER BY",
            "LIMIT",
            "OFFSET",
            "UNION",
            "UNION ALL",
            "INTERSECT",
            "EXCEPT",
            "AS",
            "WINDOW",
            "FOR UPDATE",
            "RETURNING",
            "SET",
            "USING",
            "INDEXED BY",
            "TABLESAMPLE",
        ],
        KeywordPlace::JoinWord => &["JOIN", "OUTER JOIN"],
        KeywordPlace::AfterCondition(Clause::On) => &[
            "AND",
            "OR",
            "NOT",
            "IS NULL",
            "IS NOT NULL",
            "IN",
            "LIKE",
            "BETWEEN",
            "JOIN",
            "LEFT JOIN",
            "INNER JOIN",
            "WHERE",
            "GROUP BY",
            "ORDER BY",
            "LIMIT",
        ],
        KeywordPlace::AfterCondition(Clause::Having) => &[
            "AND",
            "OR",
            "NOT",
            "IS NULL",
            "IS NOT NULL",
            "IN",
            "BETWEEN",
            "ORDER BY",
            "LIMIT",
            "UNION",
            "WINDOW",
        ],
        KeywordPlace::AfterCondition(_) => &[
            "AND",
            "OR",
            "NOT",
            "IS NULL",
            "IS NOT NULL",
            "IN",
            "LIKE",
            "ILIKE",
            "GLOB",
            "BETWEEN",
            "EXISTS",
            "SIMILAR TO",
            "ESCAPE",
            "COLLATE",
            "GROUP BY",
            "HAVING",
            "ORDER BY",
            "LIMIT",
            "OFFSET",
            "UNION",
            "UNION ALL",
            "RETURNING",
            "FOR UPDATE",
        ],
        KeywordPlace::AfterGroupItem => {
            &["HAVING", "ORDER BY", "LIMIT", "WINDOW", "UNION", "COLLATE"]
        }
        KeywordPlace::AfterOrderItem => &[
            "ASC",
            "DESC",
            "NULLS FIRST",
            "NULLS LAST",
            "LIMIT",
            "OFFSET",
            "COLLATE",
            "FETCH FIRST",
            "FOR UPDATE",
        ],
        KeywordPlace::By => &["BY"],
        KeywordPlace::Into => &["INTO"],
        KeywordPlace::From => &["FROM"],
        KeywordPlace::AfterInsertTarget => &["VALUES", "SELECT", "DEFAULT VALUES", "WITH", "AS"],
        KeywordPlace::AfterValues => &["ON CONFLICT", "RETURNING", "DO NOTHING", "DO UPDATE SET"],
        KeywordPlace::SetOperation => &["SELECT", "ALL", "DISTINCT", "VALUES"],
        KeywordPlace::AfterUpdateTarget => &["SET", "AS"],
        KeywordPlace::AfterSet => &["WHERE", "FROM", "RETURNING"],
        KeywordPlace::Other => match clause {
            Clause::Ddl => &[
                "TABLE",
                "INDEX",
                "VIEW",
                "IF EXISTS",
                "IF NOT EXISTS",
                "ADD COLUMN",
                "DROP COLUMN",
                "RENAME TO",
                "RENAME COLUMN",
                "PRIMARY KEY",
                "REFERENCES",
                "NOT NULL",
                "DEFAULT",
                "UNIQUE",
                "CHECK",
                "ON DELETE CASCADE",
                "CONSTRAINT",
                "FOREIGN KEY",
            ],
            _ => &["WHERE", "AND", "OR", "AS", "ON", "WHEN", "THEN", "ELSE", "END"],
        },
    }
}

/// The words of an expression (with the columns).
const EXPRESSION_WORDS: &[&str] = &[
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "NOT",
    "NULL",
    "TRUE",
    "FALSE",
    "EXISTS",
    "DISTINCT",
    "CAST()",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "INTERVAL",
];

/// The words of a value.
const VALUE_WORDS: &[&str] =
    &["NULL", "DEFAULT", "TRUE", "FALSE", "CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP"];

/// The functions every engine (or one) has, with their signatures: offered without a
/// connection, and their signatures given where the engine lists only names.
const BUILTINS: &[(&str, &str, Of)] = &[
    ("count", "count(expression) → integer", Of::All),
    ("sum", "sum(expression)", Of::All),
    ("avg", "avg(expression)", Of::All),
    ("min", "min(expression)", Of::All),
    ("max", "max(expression)", Of::All),
    ("coalesce", "coalesce(value, …)", Of::All),
    ("nullif", "nullif(a, b)", Of::All),
    ("lower", "lower(text) → text", Of::All),
    ("upper", "upper(text) → text", Of::All),
    ("length", "length(text) → integer", Of::All),
    ("trim", "trim(text) → text", Of::All),
    ("replace", "replace(text, from, to) → text", Of::All),
    ("round", "round(number, digits)", Of::All),
    ("abs", "abs(number)", Of::All),
    ("substr", "substr(text, start, length) → text", Of::All),
    ("ifnull", "ifnull(value, other)", Of::Sqlite),
    ("iif", "iif(condition, then, else)", Of::Sqlite),
    ("group_concat", "group_concat(expression, separator) → text", Of::Sqlite),
    ("date", "date(time, modifier, …) → text", Of::Sqlite),
    ("datetime", "datetime(time, modifier, …) → text", Of::Sqlite),
    ("strftime", "strftime(format, time, modifier, …) → text", Of::Sqlite),
    ("julianday", "julianday(time, modifier, …) → real", Of::Sqlite),
    ("json_extract", "json_extract(json, path, …)", Of::Sqlite),
    ("printf", "printf(format, …) → text", Of::Sqlite),
    ("instr", "instr(text, part) → integer", Of::Sqlite),
    ("typeof", "typeof(value) → text", Of::Sqlite),
    ("random", "random() → integer", Of::Sqlite),
    ("now", "now() → timestamp with time zone", Of::Postgres),
    ("string_agg", "string_agg(expression, delimiter) → text", Of::Postgres),
    ("array_agg", "array_agg(expression) → array", Of::Postgres),
    ("json_agg", "json_agg(expression) → json", Of::Postgres),
    ("jsonb_build_object", "jsonb_build_object(key, value, …) → jsonb", Of::Postgres),
    ("to_char", "to_char(value, format) → text", Of::Postgres),
    ("to_date", "to_date(text, format) → date", Of::Postgres),
    ("date_trunc", "date_trunc(field, source) → timestamp", Of::Postgres),
    ("extract", "extract(field FROM source) → numeric", Of::Postgres),
    ("generate_series", "generate_series(start, stop, step) → setof", Of::Postgres),
    ("concat", "concat(value, …) → text", Of::NotSqlite),
    ("substring", "substring(text FROM start FOR length) → text", Of::NotSqlite),
    ("position", "position(part IN text) → integer", Of::NotSqlite),
    ("greatest", "greatest(value, …)", Of::NotSqlite),
    ("least", "least(value, …)", Of::NotSqlite),
    ("row_number", "row_number() over (…) → bigint", Of::All),
    ("rank", "rank() over (…) → bigint", Of::All),
    ("dense_rank", "dense_rank() over (…) → bigint", Of::All),
    ("lag", "lag(value, offset, default) over (…)", Of::All),
    ("lead", "lead(value, offset, default) over (…)", Of::All),
    ("first_value", "first_value(value) over (…)", Of::All),
    ("last_value", "last_value(value) over (…)", Of::All),
    ("ifnull", "ifnull(value, other)", Of::MySql),
    ("if", "if(condition, then, else)", Of::MySql),
    ("group_concat", "group_concat(expression SEPARATOR separator) → text", Of::MySql),
    ("concat_ws", "concat_ws(separator, value, …) → text", Of::NotSqlite),
    ("date_format", "date_format(date, format) → text", Of::MySql),
    ("str_to_date", "str_to_date(text, format) → datetime", Of::MySql),
    ("date_add", "date_add(date, INTERVAL amount unit) → datetime", Of::MySql),
    ("date_sub", "date_sub(date, INTERVAL amount unit) → datetime", Of::MySql),
    ("datediff", "datediff(end, start) → integer", Of::MySql),
    ("timestampdiff", "timestampdiff(unit, start, end) → integer", Of::MySql),
    ("json_extract", "json_extract(json, path, …)", Of::MySql),
    ("json_object", "json_object(key, value, …) → json", Of::MySql),
    ("left", "left(text, length) → text", Of::NotSqlite),
    ("right", "right(text, length) → text", Of::NotSqlite),
    ("lpad", "lpad(text, length, fill) → text", Of::NotSqlite),
    ("rpad", "rpad(text, length, fill) → text", Of::NotSqlite),
    ("split_part", "split_part(text, delimiter, n) → text", Of::Postgres),
    ("regexp_replace", "regexp_replace(text, pattern, replacement, flags) → text", Of::NotSqlite),
    ("age", "age(timestamp, timestamp) → interval", Of::Postgres),
    ("make_date", "make_date(year, month, day) → date", Of::Postgres),
    ("jsonb_set", "jsonb_set(target, path, value, create_missing) → jsonb", Of::Postgres),
];

/// The signature of `name` as the engine of `flavor` has it built in, if it's one of the
/// functions known here (`substr(text, start, length) → text`).
pub fn builtin_signature(name: &str, flavor: Flavor) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|(n, _, of)| n.eq_ignore_ascii_case(name) && fits_flavor(*of, flavor))
        .map(|(_, signature, _)| *signature)
}

/// The signature of the function `name` (its parameters, what it gives): the source's own
/// or its engine's from the schema's routines when they say their arguments, else one known
/// here, else what the routine's arity says (`f(…, …)`).
pub fn signature(name: &str, flavor: Flavor, index: Option<&SchemaIndex>) -> Option<String> {
    let routine =
        index.and_then(|index| index.routines().iter().find(|r| r.name.eq_ignore_ascii_case(name)));
    match routine {
        Some(r) => Some(routine_signature(r, builtin_signature(&r.name, flavor))),
        None => builtin_signature(name, flavor).map(str::to_owned),
    }
}

/// A routine's signature: its arguments and result as the engine says, else one known
/// here, else its arity's.
fn routine_signature(r: &crate::schema_index::IndexRoutine, known: Option<&str>) -> String {
    if !r.arguments.is_empty() || r.result.is_some() {
        let result = r.result.as_deref().map(|x| format!(" → {x}")).unwrap_or_default();
        format!("{}({}){result}", r.name, r.arguments)
    } else if let Some(sig) = known {
        sig.to_owned()
    } else {
        match r.arity {
            Some(0) => format!("{}()", r.name),
            Some(n) => format!("{}({})", r.name, vec!["…"; n as usize].join(", ")),
            None => format!("{}(…)", r.name),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/completion.rs"]
mod tests;
