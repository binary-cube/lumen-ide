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
use std::sync::Arc;

use crate::TableRef;
use crate::schema_index::{IndexRelation, SchemaIndex, score};
use crate::sql::Flavor;
use crate::sql::scope::{Expect, KeywordPlace, OutputColumn, QueryScope, ScopeTable, TableSource};

mod columns;
mod functions;
mod keywords;
mod tables;
mod values;

pub use functions::{builtin_signature, signature};
use keywords::{RESERVED, VALUE_WORDS, fits, keywords_for};

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

#[cfg(test)]
#[path = "../../tests/unit/completion.rs"]
mod tests;
