//! What a statement has in scope at a place of it, for completion: the tables its `FROM`,
//! `JOIN`, `UPDATE`, `INSERT INTO`, `DELETE FROM` and `MERGE` name with their aliases, its
//! CTEs (`WITH x(a, b) AS (…)`, recursive ones too) and derived tables (`(SELECT …) t`) with
//! the columns their lists give, the columns a `USING (…)` names, and what's expected where
//! the cursor is ([`Expect`]: a keyword, a table, a column after `alias.`, a whole
//! `JOIN … ON` condition, an `INSERT`'s column list…).
//!
//! Queries nest: a subquery in `WHERE`, `IN (…)`, `EXISTS (…)` or the `SELECT` list has
//! its own scope and sees the outer one's tables (a correlated subquery); its own tables
//! don't leak out. A derived table in `FROM` sees nothing outside (unless `LATERAL`), a CTE
//! only the CTEs. Each branch of a `UNION` has its own `FROM`.
//!
//! Pure, on the statement's tokens ([`super::lex`]), and tolerant: what's typed in the
//! editor is usually unfinished (`SELECT u. FROM users u`, `SELECT FROM`, an open
//! parenthesis) — the word at the cursor is left out of what's read (a hole), so a half
//! typed name never becomes a table or swallows the next clause.

use std::ops::Range;

use super::Flavor;
use super::identifier;
use super::lex::{Token, TokenKind};
use super::resolve::{LevelScope, Name, NameRole, Resolution};
use crate::TableRef;

mod answers;
mod levels;
mod walk;

/// What fits at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// In a string, a comment, a number or a parameter, or where a new name is given (an
    /// alias after `AS`, a CTE's name): nothing to suggest.
    Nothing,
    /// The language's words fitting there.
    Keyword(KeywordPlace),
    /// A schema (`SET search_path TO |`).
    Schema,
    /// A table or a view (the statement's CTEs among them): of `schema` when one is written
    /// (`public.|`); `join`: right after `JOIN` (the tables related to those in scope first).
    Table { schema: Option<String>, join: bool },
    /// An expression: the columns of the tables in scope (those of `qualifier` only when
    /// one is written: `alias.|`, `schema.table.|`), the functions, the select list's
    /// aliases where they're allowed, the words of an expression.
    Column { qualifier: Option<Vec<String>> },
    /// Right after `JOIN t ON`: whole conditions between table `right` (the one joined,
    /// an index in [`QueryScope::tables`]) and those before it (`left`).
    JoinCondition { right: usize, left: Vec<usize> },
    /// `INSERT INTO t (|`: the columns of table `table` (an index in
    /// [`QueryScope::tables`]), all of them first.
    InsertColumns { table: usize },
    /// One table's columns, unqualified: `UPDATE t SET |`, `ON CONFLICT (|`,
    /// `CREATE INDEX … ON t (|`.
    TableColumns { table: usize },
    /// `JOIN t USING (|`: the columns both sides have.
    UsingColumns { right: usize, left: Vec<usize> },
    /// A value (`VALUES (|`, `LIMIT |`): the functions and the words for a value.
    Value,
    /// In a string literal given as a column's value (`status = 'ac|`): what that column
    /// allows ([`QueryScope::value_of`]), the literal replaced whole.
    Literal,
}

/// Where a keyword is expected: what words follow there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordPlace {
    /// A statement's first word.
    Start,
    /// After a `WITH`'s parts: the statement using them.
    AfterCte,
    /// `SELECT a |`: `FROM`, `AS`…
    AfterSelectItem,
    /// `FROM t |`, `FROM t x |` (`join`: the table was just joined: `ON`, `USING` first).
    AfterTable { join: bool },
    /// `LEFT |`: `JOIN`, `OUTER JOIN`.
    JoinWord,
    /// After a condition of `clause` (`WHERE`, `ON`, `HAVING`): `AND`, `OR`, the next
    /// clauses.
    AfterCondition(Clause),
    /// `GROUP BY a |`.
    AfterGroupItem,
    /// `ORDER BY a |`: `ASC`, `DESC`, `NULLS FIRST`…
    AfterOrderItem,
    /// `GROUP |`, `ORDER |`: `BY`.
    By,
    /// `INSERT |`, `MERGE |`: `INTO`.
    Into,
    /// `DELETE |`: `FROM`.
    From,
    /// `INSERT INTO t |` / `INSERT INTO t (a, b) |`: `VALUES`, `SELECT`, `DEFAULT VALUES`.
    AfterInsertTarget,
    /// `VALUES (…) |`: `ON CONFLICT`, `RETURNING`.
    AfterValues,
    /// `UNION |`: `ALL`, `SELECT`.
    SetOperation,
    /// `UPDATE t |`: `SET`.
    AfterUpdateTarget,
    /// `UPDATE t SET a = 1 |`: `WHERE`, `FROM`, `RETURNING`.
    AfterSet,
    /// Anywhere else.
    Other,
}

/// The clause a place is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clause {
    /// Before the statement's first word.
    #[default]
    Start,
    With,
    Select,
    From,
    /// After `JOIN`, before its `ON` / `USING`.
    Join,
    On,
    Using,
    Where,
    GroupBy,
    Having,
    Window,
    OrderBy,
    Limit,
    /// `UPDATE … SET`, `ON CONFLICT DO UPDATE SET`, `MERGE … THEN UPDATE SET`.
    Set,
    Values,
    Returning,
    /// `INSERT` up to its values.
    Insert,
    /// `UPDATE` up to its `SET`.
    Update,
    /// `DELETE` up to its `WHERE`.
    Delete,
    /// `MERGE INTO t` up to its `USING`.
    Merge,
    /// `ON CONFLICT`.
    Conflict,
    /// `SELECT … INTO new_table` (PostgreSQL): a new name.
    Into,
    /// After `UNION` / `INTERSECT` / `EXCEPT`.
    SetOperation,
    /// A schema change (`CREATE`, `DROP`, `ALTER`, `TRUNCATE`…).
    Ddl,
    /// A statement of another kind (`SET`, `PRAGMA`, `BEGIN`…).
    Other,
}

/// A column a CTE or a derived table gives: named (its alias, or the column it is), or
/// every column of a table (`*`, `t.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputColumn {
    Named(String),
    /// `*` or `t.*` (`qualifier`): every column of `tables` (its query's database tables
    /// it means), resolved through the schema.
    Star {
        qualifier: Option<String>,
        tables: Vec<TableRef>,
    },
}

/// Where a table in scope comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableSource {
    /// A table or view of the database (its name as written, unquoted).
    Table(TableRef),
    /// One of the statement's CTEs, with its columns (its list's, else its query's).
    Cte { name: String, columns: Vec<OutputColumn> },
    /// `(SELECT …) t`: the columns of its select list.
    Derived { columns: Vec<OutputColumn> },
    /// `generate_series(1, 3) g`: a function's rows (their columns unknown).
    Function(String),
}

/// A table in scope at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeTable {
    pub source: TableSource,
    pub alias: Option<String>,
    /// How far out it is: 0 at the cursor's own query, 1 the query around it…
    pub level: usize,
    /// Where it's named (bytes of the text).
    pub range: Range<usize>,
}

impl ScopeTable {
    /// What it's called in the statement: its alias, else its name.
    pub fn name(&self) -> &str {
        if let Some(alias) = &self.alias {
            return alias;
        }
        match &self.source {
            TableSource::Table(t) => &t.name,
            TableSource::Cte { name, .. } | TableSource::Function(name) => name,
            TableSource::Derived { .. } => "",
        }
    }

    /// The database's table it is (none for a CTE, a derived table, a function).
    pub fn table(&self) -> Option<&TableRef> {
        match &self.source {
            TableSource::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Whether `qualifier` (`alias`, `table`, `schema.table`) names it: its alias, else its
    /// name (any case, as the engines compare unquoted names).
    pub fn answers_to(&self, qualifier: &[String]) -> bool {
        match qualifier {
            [one] => {
                self.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(one))
                    || (self.alias.is_none() && self.name().eq_ignore_ascii_case(one))
                    || matches!(&self.source, TableSource::Table(t)
                        if self.alias.is_some() && t.name.eq_ignore_ascii_case(one))
            }
            [.., schema, name] => match &self.source {
                TableSource::Table(t) => {
                    t.name.eq_ignore_ascii_case(name)
                        && t.schema.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(schema))
                }
                _ => false,
            },
            [] => false,
        }
    }
}

/// A CTE of the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cte {
    pub name: String,
    pub columns: Vec<OutputColumn>,
    /// Where its name is written.
    pub range: Range<usize>,
}

/// A `*` of a select list right before the cursor: what "expand the column list" replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Star {
    /// The `*` (with its `t.` when qualified).
    pub range: Range<usize>,
    pub qualifier: Option<String>,
}

/// What's known at the cursor of a statement (see the module's doc).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueryScope {
    pub expect: Expect,
    /// The word being typed (what a suggestion replaces): from its start to the cursor.
    pub replace: Range<usize>,
    /// That word, unquoted.
    pub prefix: String,
    /// It's typed with an opening quote (`"Us|`).
    pub quoted: bool,
    pub clause: Clause,
    /// The tables in scope: the cursor's query's first (in the order named), then the
    /// outer ones'.
    pub tables: Vec<ScopeTable>,
    /// The CTEs visible there.
    pub ctes: Vec<Cte>,
    /// The select list's aliases usable there (`ORDER BY`, and `GROUP BY` / `HAVING` where
    /// the dialect allows).
    pub aliases: Vec<String>,
    /// A `*` right before the cursor in a select list.
    pub star: Option<Star>,
    /// Every table of the database the statement names (at any level): whose columns it
    /// needs.
    pub named: Vec<TableRef>,
    /// The column whose value is written at the cursor (`status = |`, `IN (|`, an
    /// `INSERT`'s values): its enum's literals are offered.
    pub value_of: Option<super::value::ValueOf>,
}

impl Default for Expect {
    fn default() -> Self {
        Expect::Keyword(KeywordPlace::Start)
    }
}

impl QueryScope {
    /// The tables `qualifier` names (an alias, a table, `schema.table`).
    pub fn qualified(&self, qualifier: &[String]) -> Vec<usize> {
        // The innermost one answering wins (an alias hides an outer one of the same name).
        let mut found: Vec<usize> =
            (0..self.tables.len()).filter(|&i| self.tables[i].answers_to(qualifier)).collect();
        if let Some(level) = found.iter().map(|&i| self.tables[i].level).min() {
            found.retain(|&i| self.tables[i].level == level);
        }
        found
    }
}

/// What's in scope at `offset` of `text` (bytes): the statement around it read from
/// `tokens` (the whole text's, [`super::lex`]), by `flavor`'s rules.
pub fn scope_in(text: &str, tokens: &[Token], offset: usize, flavor: Flavor) -> QueryScope {
    let statements = super::split(text, tokens, flavor, &super::SplitOptions::default());
    // The last statement starting before the cursor, unless its `;` is behind it too.
    let at = statements.iter().rposition(|s| s.range.start < offset);
    let range = match at.map(|i| &statements[i]) {
        Some(s) if s.terminator.as_ref().is_none_or(|t| t.start >= offset) => s.tokens.clone(),
        Some(s) => {
            let next = s.tokens.end + usize::from(s.terminator.is_some());
            next..next
        }
        None => 0..0,
    };
    // Up to the cursor's token at least (what's after the statement's last token).
    let end = tokens.partition_point(|t| t.start < offset).max(range.end);
    let start = range.start.min(end);
    let statement = &tokens[start..end.min(tokens.len())];
    // In a string literal: what's in scope where it starts, its column's values offered.
    if let Some(literal) = super::value::open_literal(text, statement, offset) {
        let Some(value_of) = super::value::value_of(text, statement, literal.start) else {
            return scope(text, statement, tokens, offset, flavor);
        };
        let mut found = scope(text, statement, tokens, literal.start, flavor);
        found.expect = Expect::Literal;
        found.replace = super::value::literal_range(&literal, offset);
        found.prefix = text[literal.start + 1..offset].to_owned();
        found.quoted = false;
        found.star = None;
        found.value_of = Some(value_of);
        return found;
    }
    let mut found = scope(text, statement, tokens, offset, flavor);
    if matches!(found.expect, Expect::Column { qualifier: None } | Expect::Value) {
        found.value_of = super::value::value_of(text, statement, found.replace.start);
    }
    found
}

/// What's in scope at `offset` of one statement (`statement`: its tokens; `all`: the
/// text's, to tell whether the cursor is inside a string or a comment).
pub fn scope(
    text: &str,
    statement: &[Token],
    all: &[Token],
    offset: usize,
    flavor: Flavor,
) -> QueryScope {
    let offset = offset.min(text.len());
    // Inside a string, a comment, a body, a number, a parameter: nothing.
    let inside = all.iter().find(|t| {
        (t.start < offset && offset < t.end)
            || (t.start < offset
                && t.end == offset
                && (t.unterminated
                    || matches!(
                        t.kind,
                        TokenKind::LineComment | TokenKind::Number | TokenKind::Parameter(_)
                    )))
    });
    if let Some(t) = inside
        && matches!(
            t.kind,
            TokenKind::LineComment
                | TokenKind::BlockComment
                | TokenKind::String(_)
                | TokenKind::DollarBody
                | TokenKind::Number
                | TokenKind::Parameter(_)
        )
    {
        return QueryScope {
            expect: Expect::Nothing,
            replace: offset..offset,
            ..QueryScope::default()
        };
    }
    let ends_here = |t: &Token| {
        t.start < offset
            && offset <= t.end
            && matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent)
    };
    // The word being typed (the hole), and the statement's other significant tokens.
    let typed = all.iter().find(|t| ends_here(t)).copied();
    let (replace, prefix, quoted) = match typed {
        Some(t) => {
            let raw = &text[t.start..offset];
            let quoted = t.kind == TokenKind::QuotedIdent;
            let inner = if quoted {
                raw.trim_start_matches(['"', '`', '[']).trim_start_matches("U&").to_owned()
            } else {
                raw.to_owned()
            };
            (t.start..offset, inner, quoted)
        }
        None => (offset..offset, String::new(), false),
    };
    let hole_at = replace.start;
    let mut sig: Vec<Token> = Vec::with_capacity(statement.len() / 2 + 2);
    let mut hole = None;
    for &t in statement.iter().filter(|t| !t.is_trivia()) {
        if Some(t) == typed {
            continue;
        }
        if hole.is_none() && t.start >= hole_at {
            hole = Some(sig.len());
            sig.push(Token {
                kind: TokenKind::Punct,
                start: hole_at,
                end: hole_at,
                unterminated: false,
            });
        }
        sig.push(t);
    }
    let hole = hole.unwrap_or_else(|| {
        sig.push(Token {
            kind: TokenKind::Punct,
            start: hole_at,
            end: hole_at,
            unterminated: false,
        });
        sig.len() - 1
    });
    let mut scan = Scan::new(text, flavor, sig, hole);
    scan.run();
    let mut out = scan.result(offset);
    out.replace = replace;
    out.prefix = prefix;
    out.quoted = quoted;
    out
}

/// What a level's `FROM` reads next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FromState {
    #[default]
    None,
    /// A table (after `FROM`, `JOIN`, `,`, `INTO`, `UPDATE`…).
    ExpectTable,
    /// After a table's name (its alias may follow).
    AfterTable,
    /// After `AS`: the alias.
    ExpectAlias,
    /// After the alias.
    AfterAlias,
    /// `LEFT`, `INNER`…: `JOIN` follows.
    JoinPrefix,
    /// After `INSERT INTO t (…)`.
    AfterColumns,
}

/// Where a `WITH`'s parts are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CteState {
    #[default]
    None,
    ExpectName,
    /// After its name (its column list or `AS`).
    AfterName,
    /// After `AS`: its query.
    ExpectBody,
    /// After its query: `,` or the statement.
    AfterBody,
}

/// What a schema change waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Ddl {
    #[default]
    None,
    /// `CREATE`: a new name follows (nothing to suggest), unless an index's `ON`.
    Create,
    /// `CREATE [UNIQUE] INDEX`: its `ON` names a table.
    Index,
    /// `DROP`, `ALTER`, `TRUNCATE`, `ANALYZE`…: an existing table follows.
    Existing,
}

/// The state of a query level as its tokens are read.
#[derive(Debug, Clone, Default)]
struct State {
    clause: Clause,
    from: FromState,
    /// The table just joined (its index in the level's tables), until the next one.
    join: Option<usize>,
    /// The table an `INSERT`, `UPDATE`, `DELETE` or `MERGE` changes.
    target: Option<usize>,
    /// Where the select list's current item starts.
    item: usize,
    /// The select list's items are this level's outputs (its first branch's).
    collecting: bool,
    cte: CteState,
    ddl: Ddl,
    /// `GROUP` / `ORDER` / `PARTITION` read: `BY` gives this clause.
    by: Option<Clause>,
    /// In `SET`, left of its `=`: the column set.
    set_left: bool,
    /// `LATERAL` read: the next derived table sees the level's tables.
    lateral: bool,
    /// The statement's first word was `DELETE`, `MERGE`, `UPDATE`…
    statement: Clause,
    /// `SET search_path TO`: a schema follows.
    search_path: bool,
    /// The level's `VALUES` row lists are done (`VALUES (…) |`).
    values_done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LevelKind {
    Top,
    /// A CTE's query (sees only the CTEs).
    Cte,
    /// `(SELECT …) t` in `FROM`: sees nothing outside, unless `LATERAL`.
    Derived {
        lateral: bool,
    },
    /// In an expression (`WHERE`, `IN (…)`, `EXISTS (…)`, the select list): sees the
    /// outer tables.
    Nested,
}

#[derive(Debug, Clone)]
struct Level {
    parent: Option<usize>,
    kind: LevelKind,
    tables: Vec<ScopeTable>,
    ctes: Vec<Cte>,
    outputs: Vec<OutputColumn>,
    aliases: Vec<String>,
    /// The first branch of its `UNION` (whose select list names the columns).
    first: usize,
    /// Where it starts (its `(`, bytes).
    open: usize,
    /// How many tables its parent had when it opened (a `LATERAL` one sees those).
    before: usize,
    /// It joins a table by `USING (…)` or `NATURAL` (a column both have isn't ambiguous).
    using: bool,
    state: State,
}

/// A parenthesis that isn't a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Group {
    /// An expression's parentheses, an `IN (…)` list, a window's `OVER (…)`.
    Expr,
    /// A function's arguments.
    Call,
    /// `INSERT INTO t (…)`: table `usize` of the level.
    InsertColumns(usize),
    /// `ON CONFLICT (…)`, `CREATE INDEX … ON t (…)`: one table's columns.
    TableColumns(usize),
    /// `JOIN … USING (…)`.
    Using,
    /// `WITH x (a, b)`: new names.
    CteColumns,
    /// `VALUES (…)`.
    Values,
    /// A `CREATE TABLE`'s definitions: nothing to suggest.
    Definitions,
    /// `FROM (a JOIN b)`: read as the level's own.
    From,
}

#[derive(Debug, Clone, Copy)]
enum Frame {
    Level(usize),
    Group(Group),
}

/// The scan's state at the hole.
#[derive(Debug, Clone)]
struct Snapshot {
    level: usize,
    state: State,
    group: Option<Group>,
}

struct Scan<'a> {
    text: &'a str,
    flavor: Flavor,
    sig: Vec<Token>,
    hole: usize,
    levels: Vec<Level>,
    frames: Vec<Frame>,
    snapshot: Option<Snapshot>,
    /// The CTE whose query is being read (level, index in its ctes), by frame.
    pending_cte: Vec<(usize, usize)>,
    /// What each name was read as ([`super::resolve`]; kept only when asked).
    seen: Option<Vec<Seen>>,
}

/// What a name of the statement was read as, where (for [`super::resolve`]).
#[derive(Debug, Clone)]
struct Seen {
    /// Its index among the significant tokens.
    at: usize,
    level: usize,
    role: Role,
    /// Its level's statement is a schema change (`DROP TABLE t`, `CREATE INDEX … ON t`).
    ddl: bool,
}

#[derive(Debug, Clone, Copy)]
enum Role {
    /// A table's name (`table`: its index in the level's tables).
    Table(usize),
    /// The schema (or database) before a table's name.
    Schema(usize),
    Alias(usize),
    /// A CTE's name where it's defined.
    Cte,
    /// A name in an expression (or a parenthesis' list).
    Expr {
        clause: Clause,
        group: Option<Group>,
        set_left: bool,
        target: Option<usize>,
    },
}

/// Words that can't be a table's alias.
const NOT_ALIAS: &[&str] = &[
    "AS",
    "SET",
    "VALUES",
    "DEFAULT",
    "SELECT",
    "FROM",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "AND",
    "OR",
    "DO",
    "OVER",
    "INTO",
    "OUTER",
    "LATERAL",
    "USING",
    "RETURNING",
    "WHERE",
    "GROUP",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "FOR",
    "WINDOW",
    "HAVING",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "NATURAL",
    "STRAIGHT_JOIN",
    "ON",
    "TABLESAMPLE",
    "INDEXED",
    "NOT",
    "WITH",
    "ONLY",
    "MATCHED",
    "CONFLICT",
];

/// Words after which an expression goes on (a column fits).
const CONTINUES: &[&str] = &[
    "SELECT",
    "DISTINCT",
    "ALL",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "ON",
    "WHEN",
    "THEN",
    "ELSE",
    "CASE",
    "BY",
    "HAVING",
    "SET",
    "IN",
    "LIKE",
    "ILIKE",
    "GLOB",
    "REGEXP",
    "MATCH",
    "BETWEEN",
    "IS",
    "EXISTS",
    "RETURNING",
    "ANY",
    "SOME",
    "ESCAPE",
    "VALUES",
    "SIMILAR",
    "TO",
    "OVER",
    "PARTITION",
    "USING",
    "DESC",
    "ASC",
    "WITHIN",
    "FILTER",
    "COLLATE",
    "IF",
    "DO",
];

/// Words that are never a name in an expression (unquoted).
const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "TRUE",
    "FALSE",
    "IS",
    "IN",
    "LIKE",
    "ILIKE",
    "GLOB",
    "REGEXP",
    "BETWEEN",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "AS",
    "ON",
    "USING",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "NATURAL",
    "DISTINCT",
    "ALL",
    "ANY",
    "SOME",
    "EXISTS",
    "ASC",
    "DESC",
    "NULLS",
    "LIMIT",
    "OFFSET",
    "GROUP",
    "ORDER",
    "BY",
    "HAVING",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "INTERVAL",
    "CAST",
    "COLLATE",
    "ESCAPE",
    "OVER",
    "PARTITION",
    "FILTER",
    "WITHIN",
    "SET",
    "VALUES",
    "DEFAULT",
    "RETURNING",
    "INTO",
    "WITH",
    "RECURSIVE",
    "LATERAL",
    "SIMILAR",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "CURRENT_USER",
    "SESSION_USER",
    "ARRAY",
    "UNBOUNDED",
    "PRECEDING",
    "FOLLOWING",
    "WINDOW",
    "FETCH",
    "UPDATE",
    "DELETE",
    "INSERT",
    "MERGE",
    "MATCHED",
    "DO",
    "NOTHING",
    "CONFLICT",
];

/// Words after which a bare name is surely an operand.
const OPERAND_AFTER: &[&str] = &[
    "SELECT",
    "DISTINCT",
    "ALL",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "ON",
    "BY",
    "HAVING",
    "WHEN",
    "THEN",
    "ELSE",
    "CASE",
    "SET",
    "RETURNING",
    "IN",
    "LIKE",
    "ILIKE",
    "GLOB",
    "REGEXP",
    "BETWEEN",
    "ESCAPE",
];

/// Every name of `statement` (its tokens, of `text`) with what it is (see
/// [`super::resolve`]).
pub(super) fn names(text: &str, statement: &[Token], flavor: Flavor) -> Resolution {
    let sig: Vec<Token> = statement.iter().filter(|t| !t.is_trivia()).copied().collect();
    let mut scan = Scan::new(text, flavor, sig, usize::MAX);
    scan.seen = Some(Vec::new());
    scan.run();
    scan.resolution()
}

impl<'a> Scan<'a> {
    fn new(text: &'a str, flavor: Flavor, sig: Vec<Token>, hole: usize) -> Self {
        let top = Level {
            parent: None,
            kind: LevelKind::Top,
            tables: Vec::new(),
            ctes: Vec::new(),
            outputs: Vec::new(),
            aliases: Vec::new(),
            first: 0,
            open: 0,
            before: 0,
            using: false,
            state: State { collecting: true, ..State::default() },
        };
        Self {
            text,
            flavor,
            sig,
            hole,
            levels: vec![top],
            frames: vec![Frame::Level(0)],
            snapshot: None,
            pending_cte: Vec::new(),
            seen: None,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/scope.rs"]
mod tests;
