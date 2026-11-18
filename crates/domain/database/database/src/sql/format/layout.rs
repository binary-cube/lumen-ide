//! A statement's layout: its tokens (those that say something, each with the comments
//! around it) read into clauses, lists, conditions, parentheses and `CASE` blocks, made into
//! a [`Doc`]. Only where lines break and spaces go is decided here; every token is written as
//! it is (a keyword in the case chosen).

use std::borrow::Cow;
use std::ops::Range;

use super::doc::Doc;
use super::words;
use super::{CommaStyle, FormatOptions, KeywordCase, ListStyle};
use crate::sql::{Flavor, Token, TokenKind};

/// Deeper than this (parentheses, brackets), a statement is left as written: nobody writes
/// it, and the layout would only nest as deep.
pub(super) const MAX_DEPTH: usize = 100;

const NONE: usize = usize::MAX;

/// A comment beside a token.
#[derive(Debug, Clone, Copy)]
struct Note<'a> {
    text: &'a str,
    /// A line comment (`--`): the line ends with it.
    line: bool,
}

/// A token that says something, with the comments around it.
#[derive(Debug, Clone)]
struct Sig<'a> {
    kind: TokenKind,
    text: &'a str,
    /// Its text uppercase, for a word (what it's compared by).
    upper: Option<Box<str>>,
    /// Nothing between it and the token before (no space, no comment).
    glued: bool,
    /// A line break between it and the token before.
    newline: bool,
    /// Comments on lines of their own before it.
    leading: Vec<Note<'a>>,
    /// Comments after it on its line.
    trailing: Vec<Note<'a>>,
    /// Written as a keyword (in the case chosen).
    keyword: bool,
}

/// How clauses of one query follow each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sep {
    /// Each on a line of its own (a statement, a CTE's body).
    Hard,
    /// On one line if the parenthesis around fits (a subquery).
    Line,
}

impl Sep {
    fn doc<'a>(self) -> Doc<'a> {
        match self {
            Sep::Hard => Doc::Hard,
            Sep::Line => Doc::Line,
        }
    }
}

/// What a clause holds, which says how it's laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// What comes before the first clause (a parenthesized query before `UNION`).
    Lead,
    /// A list (`SELECT`, `GROUP BY`, `SET`, `VALUES`…): on its line, or one item a line.
    List,
    /// A condition (`WHERE`, `HAVING`): broken before `AND` / `OR`.
    Cond,
    /// The rest on the clause's line (`LIMIT`, `INSERT INTO`, a join).
    Plain,
    /// `UNION`, `EXCEPT`, `INTERSECT`: alone on its line.
    SetOp,
    /// `WITH` and its CTEs.
    With,
    /// A join's `ON` (or `MERGE`'s): on the clause's line, or the next, indented.
    On,
}

#[derive(Debug, Clone)]
struct Clause {
    kind: Kind,
    head: Range<usize>,
    body: Range<usize>,
}

/// One statement being laid out.
pub(super) struct Layout<'a, 'o> {
    sig: Vec<Sig<'a>>,
    /// A `(` / `[`'s closing one (`NONE`: not closed, or not one).
    close: Vec<usize>,
    /// A `CASE`'s `END` (`NONE`: not closed, or not one).
    end_of: Vec<usize>,
    flavor: Flavor,
    options: &'o FormatOptions,
}

impl<'a, 'o> Layout<'a, 'o> {
    /// The statement of `tokens` (of `text`); `None` when it's left as written (too deep, a
    /// psql command — `\` ends at its line).
    pub(super) fn new(
        text: &'a str,
        tokens: &[Token],
        flavor: Flavor,
        options: &'o FormatOptions,
    ) -> Option<Self> {
        let sig = read(text, tokens)?;
        let mut layout = Self {
            close: vec![NONE; sig.len()],
            end_of: vec![NONE; sig.len()],
            sig,
            flavor,
            options,
        };
        layout.match_pairs()?;
        layout.mark_keywords();
        layout.mark_types();
        Some(layout)
    }

    /// The statement's layout; its last token is a separator line (`GO`, `/`) when
    /// `separator_line`, else maybe its `;`.
    pub(super) fn statement(&mut self, separator_line: bool) -> Doc<'a> {
        let n = self.sig.len();
        let last = n.checked_sub(1);
        let terminator = match last {
            Some(last) if separator_line => Some((last, true)),
            Some(last) if self.sig[last].kind == TokenKind::Semicolon => Some((last, false)),
            _ => None,
        };
        let end = terminator.map_or(n, |(t, _)| t);
        let mut docs = vec![self.stmt(0..end)];
        match terminator {
            Some((t, true)) => docs.extend([Doc::Hard, self.tok(t)]),
            Some((t, false)) => docs.push(self.tok(t)),
            None => {}
        }
        Doc::seq(docs)
    }

    // ---- reading ----

    fn match_pairs(&mut self) -> Option<()> {
        let mut open: Vec<usize> = Vec::new();
        let mut cases: Vec<(usize, usize)> = Vec::new();
        for i in 0..self.sig.len() {
            match (self.sig[i].kind, self.sig[i].text) {
                (TokenKind::OpenParen, _) | (TokenKind::Punct, "[") => {
                    open.push(i);
                    if open.len() > MAX_DEPTH {
                        return None;
                    }
                }
                (TokenKind::CloseParen, _) | (TokenKind::Punct, "]") => {
                    let closing_paren = self.sig[i].kind == TokenKind::CloseParen;
                    let fits = open.last().is_some_and(|&o| {
                        (self.sig[o].kind == TokenKind::OpenParen) == closing_paren
                    });
                    if fits && let Some(o) = open.pop() {
                        self.close[o] = i;
                        // A `CASE` left open inside is never closed.
                        cases.retain(|&(_, depth)| depth <= open.len());
                    }
                }
                (TokenKind::Word, _) => match self.name_free(i) {
                    Some("CASE") if self.prev_upper(i) != Some("END") => {
                        cases.push((i, open.len()));
                    }
                    Some("END") => {
                        if cases.last().is_some_and(|&(_, depth)| depth == open.len())
                            && let Some((case, _)) = cases.pop()
                        {
                            self.end_of[case] = i;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Some(())
    }

    /// Which words are written as keywords.
    fn mark_keywords(&mut self) {
        for i in 0..self.sig.len() {
            let Some(upper) = self.name_free(i) else { continue };
            let previous = self.prev_upper(i);
            // An aggregate's `FILTER (WHERE …)`.
            let filter =
                upper == "FILTER" && self.kind_at(i.wrapping_sub(1)) == Some(TokenKind::CloseParen);
            let word = words::keyword(upper)
                || filter
                || previous.is_some_and(|p| words::keyword_after(upper, p));
            let keyword = if self.kind_at(i.wrapping_sub(1)) == Some(TokenKind::Cast) {
                words::type_name(upper)
            } else if self.kind_at(i + 1) == Some(TokenKind::OpenParen) {
                word && (words::before_paren(upper) || words::call_like(upper))
            } else {
                word
            };
            self.sig[i].keyword = keyword;
        }
        // The rest of a type after `::` (`::double precision`).
        for i in 1..self.sig.len() {
            if self.sig[i - 1].keyword
                && self.kind_at(i.wrapping_sub(2)) == Some(TokenKind::Cast)
                && self.name_free(i).is_some_and(words::type_name)
            {
                self.sig[i].keyword = true;
            }
        }
    }

    /// Types where a type goes: a column's in `CREATE TABLE` and `ADD COLUMN`, after
    /// `TYPE`, `RETURNS`, and in `CAST(… AS type)`.
    fn mark_types(&mut self) {
        let n = self.sig.len();
        if let Some(p) = self.table_columns(0..n) {
            let close = self.close[p];
            let (items, _) = self.split_commas(p + 1..close);
            for item in items {
                let first = self.name_free(item.start);
                let constraint = first.is_some_and(|w| {
                    matches!(
                        w,
                        "CONSTRAINT"
                            | "PRIMARY"
                            | "UNIQUE"
                            | "CHECK"
                            | "FOREIGN"
                            | "EXCLUDE"
                            | "LIKE"
                            | "INDEX"
                            | "KEY"
                    )
                });
                if !item.is_empty() && self.sig[item.start].is_name() && !constraint {
                    self.type_at(item.start + 1, item.end);
                }
            }
        }
        for i in 0..n {
            match self.name_free(i) {
                Some("TYPE") => {
                    // `ALTER COLUMN c TYPE text`, `CREATE TYPE`: a keyword there.
                    let column = i >= 2 && self.name_free(i - 2) == Some("COLUMN");
                    if column || matches!(self.prev_upper(i), Some("CREATE" | "ALTER" | "DROP")) {
                        self.sig[i].keyword = true;
                    }
                    self.type_at(i + 1, n);
                }
                Some("RETURNS") => self.type_at(i + 1, n),
                Some("COLUMN") if self.prev_upper(i) == Some("ADD") => {
                    let mut j = i + 1;
                    if self.name_free(j) == Some("IF") {
                        j += 3;
                    }
                    self.type_at(j + 1, n);
                }
                Some("CAST") if self.close.get(i + 1).is_some_and(|&c| c != NONE) => {
                    let close = self.close[i + 1];
                    let mut j = i + 2;
                    let mut as_at = None;
                    while j < close {
                        if self.name_free(j) == Some("AS") {
                            as_at = Some(j);
                        }
                        j = self.skip(j, close);
                    }
                    if let Some(a) = as_at {
                        self.type_at(a + 1, close);
                    }
                }
                _ => {}
            }
        }
    }

    /// The type's words starting at `at` (before `end`) written as keywords.
    fn type_at(&mut self, at: usize, end: usize) {
        let mut j = at;
        while j < end && self.name_free(j).is_some_and(words::type_name) {
            self.sig[j].keyword = true;
            j += 1;
        }
    }

    fn kind_at(&self, i: usize) -> Option<TokenKind> {
        self.sig.get(i).map(|s| s.kind)
    }

    /// The word at `i`, uppercase, if it isn't a qualified name's part (`t.order`).
    fn name_free(&self, i: usize) -> Option<&str> {
        let s = self.sig.get(i)?;
        let dotted = |j: usize| self.kind_at(j) == Some(TokenKind::Dot);
        if s.kind != TokenKind::Word || dotted(i.wrapping_sub(1)) || dotted(i + 1) {
            return None;
        }
        s.upper.as_deref()
    }

    /// The word right before `i`, uppercase.
    fn prev_upper(&self, i: usize) -> Option<&str> {
        self.sig.get(i.checked_sub(1)?)?.upper.as_deref()
    }

    /// Past the group starting at `j` (a parenthesis, a bracket, a `CASE` block) if it
    /// ends before `end`, else past `j`.
    fn skip(&self, j: usize, end: usize) -> usize {
        let close = self.close[j];
        if close != NONE && close < end {
            return close + 1;
        }
        let case_end = self.end_of[j];
        if case_end != NONE && case_end < end {
            return case_end + 1;
        }
        j + 1
    }

    /// The items of `r` between its commas at its own level, and the commas.
    fn split_commas(&self, r: Range<usize>) -> (Vec<Range<usize>>, Vec<usize>) {
        let mut items = Vec::new();
        let mut commas = Vec::new();
        let mut start = r.start;
        let mut j = r.start;
        while j < r.end {
            if self.sig[j].kind == TokenKind::Comma {
                items.push(start..j);
                commas.push(j);
                start = j + 1;
            }
            j = self.skip(j, r.end);
        }
        items.push(start..r.end);
        (items, commas)
    }

    // ---- statements ----

    fn stmt(&mut self, r: Range<usize>) -> Doc<'a> {
        if r.is_empty() {
            return Doc::seq(Vec::new());
        }
        match self.name_free(r.start) {
            Some(
                "SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES" | "MERGE" | "REPLACE",
            ) => self.query(r, Sep::Hard),
            Some("EXPLAIN") => self.explain(r),
            Some("CREATE") => self.create(r),
            _ if self.paren_is_query(r.start) => self.query(r, Sep::Hard),
            _ => self.inline(r),
        }
    }

    /// `EXPLAIN …` on its line, the statement explained under it.
    fn explain(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut j = r.start + 1;
        while j < r.end {
            if matches!(
                self.name_free(j),
                Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES" | "MERGE")
            ) {
                let head = self.expr(r.start..j);
                return Doc::seq(vec![head, Doc::Hard, self.query(j..r.end, Sep::Hard)]);
            }
            j = self.skip(j, r.end);
        }
        self.inline(r)
    }

    fn create(&mut self, r: Range<usize>) -> Doc<'a> {
        if let Some(begin) = self.body_begin(r.clone()) {
            return self.create_with_body(r, begin);
        }
        if let Some(p) = self.table_columns(r.clone()) {
            return self.create_table(r, p);
        }
        if let Some(a) = self.as_query(r.clone()) {
            let head = self.expr(r.start..a + 1);
            return Doc::seq(vec![head, Doc::Hard, self.query(a + 1..r.end, Sep::Hard)]);
        }
        self.inline(r)
    }

    /// A trigger's `BEGIN … END` body (or a `BEGIN ATOMIC` one) holding statements of its
    /// own, ending the statement: where its `BEGIN` is.
    fn body_begin(&self, r: Range<usize>) -> Option<usize> {
        let last = r.end.checked_sub(1)?;
        if self.name_free(last) != Some("END") {
            return None;
        }
        let trigger =
            (r.start..r.end.min(r.start + 6)).any(|j| self.name_free(j) == Some("TRIGGER"));
        let mut begin = None;
        let mut j = r.start + 1;
        while j < last {
            if self.name_free(j) == Some("BEGIN") {
                if begin.is_some() {
                    // Blocks inside (MySQL's): left on their lines as they are.
                    return None;
                }
                let atomic = self.name_free(j + 1) == Some("ATOMIC");
                if (atomic && self.flavor.begin_atomic()) || (trigger && !self.flavor.is_postgres())
                {
                    begin = Some(j);
                } else {
                    return None;
                }
            }
            j = self.skip(j, last);
        }
        begin
    }

    fn create_with_body(&mut self, r: Range<usize>, begin: usize) -> Doc<'a> {
        let end = r.end - 1;
        let atomic = self.name_free(begin + 1) == Some("ATOMIC");
        let mut docs = vec![self.inline(r.start..begin), Doc::Hard, self.tok(begin)];
        if atomic {
            docs.extend([Doc::Space, self.tok(begin + 1)]);
        }
        let mut inner = Vec::new();
        let mut start = begin + 1 + usize::from(atomic);
        let mut j = start;
        while j < end {
            if self.sig[j].kind == TokenKind::Semicolon {
                inner.extend([Doc::Hard, self.stmt(start..j), self.tok(j)]);
                start = j + 1;
            }
            j = self.skip(j, end);
        }
        if start < end {
            inner.extend([Doc::Hard, self.stmt(start..end)]);
        }
        docs.extend([Doc::indent(inner), Doc::Hard, self.tok(end)]);
        Doc::seq(docs)
    }

    /// `CREATE … TABLE [IF NOT EXISTS] name (`: where its columns' `(` is.
    fn table_columns(&self, r: Range<usize>) -> Option<usize> {
        if self.name_free(r.start) != Some("CREATE") {
            return None;
        }
        let mut j = r.start + 1;
        while matches!(
            self.name_free(j),
            Some("OR" | "REPLACE" | "TEMP" | "TEMPORARY" | "UNLOGGED" | "GLOBAL" | "LOCAL")
        ) {
            j += 1;
        }
        if self.name_free(j) != Some("TABLE") {
            return None;
        }
        j += 1;
        if self.name_free(j) == Some("IF") {
            j += 3;
        }
        if !self.sig.get(j)?.is_name() {
            return None;
        }
        j += 1;
        while self.kind_at(j) == Some(TokenKind::Dot) && self.sig.get(j + 1)?.is_name() {
            j += 2;
        }
        let close = *self.close.get(j)?;
        (self.kind_at(j) == Some(TokenKind::OpenParen) && close != NONE && close < r.end)
            .then_some(j)
    }

    /// `CREATE TABLE t (` with a column (or a constraint) a line.
    fn create_table(&mut self, r: Range<usize>, p: usize) -> Doc<'a> {
        let close = self.close[p];
        let head = self.expr(r.start..p);
        let (items, commas) = self.split_commas(p + 1..close);
        let mut inner = vec![Doc::Hard];
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                let comma = self.tok(commas[k - 1]);
                match self.options.commas {
                    CommaStyle::Trailing => inner.extend([comma, Doc::Hard]),
                    CommaStyle::Leading => inner.extend([Doc::Hard, comma, Doc::Space]),
                }
            }
            inner.push(self.expr(item));
        }
        let mut docs =
            vec![head, Doc::Space, self.tok(p), Doc::indent(inner), Doc::Hard, self.tok(close)];
        let rest = close + 1..r.end;
        if !rest.is_empty() {
            match self.as_query(rest.clone()) {
                Some(a) if a == rest.start => {
                    docs.extend([Doc::Space, self.tok(a), Doc::Hard]);
                    docs.push(self.query(a + 1..r.end, Sep::Hard));
                }
                _ => docs.extend([Doc::Space, self.inline(rest)]),
            }
        }
        Doc::seq(docs)
    }

    /// An `AS` at the top level followed by a query (`CREATE VIEW v AS SELECT`).
    fn as_query(&self, r: Range<usize>) -> Option<usize> {
        let mut j = r.start;
        while j < r.end {
            if self.name_free(j) == Some("AS")
                && j + 1 < r.end
                && (matches!(self.name_free(j + 1), Some("SELECT" | "WITH" | "VALUES"))
                    || self.paren_is_query(j + 1))
            {
                return Some(j);
            }
            j = self.skip(j, r.end);
        }
        None
    }

    /// Any other statement: on its line, broken after its commas when too long.
    fn inline(&mut self, r: Range<usize>) -> Doc<'a> {
        let (items, commas) = self.split_commas(r.clone());
        if commas.is_empty() {
            return self.expr(r);
        }
        let mut items = items.into_iter();
        let first = items.next().map(|i| self.expr(i)).unwrap_or_else(|| Doc::seq(Vec::new()));
        let mut rest = Vec::new();
        for (k, item) in items.enumerate() {
            self.separator(&mut rest, commas[k], Doc::Line);
            rest.push(self.expr(item));
        }
        Doc::group(vec![first, Doc::indent(rest)])
    }

    /// A comma between items, its break after it (or before it, commas leading).
    fn separator(&self, docs: &mut Vec<Doc<'a>>, comma: usize, line: Doc<'a>) {
        // After a line comment the comma starts the next line: the item follows it there.
        let after_comment = comma
            .checked_sub(1)
            .is_some_and(|p| self.sig[p].trailing.last().is_some_and(|n| n.line));
        let comma = self.tok(comma);
        match self.options.commas {
            CommaStyle::Trailing if after_comment => docs.extend([comma, Doc::Space]),
            CommaStyle::Trailing => docs.extend([comma, line]),
            CommaStyle::Leading => docs.extend([line, comma, Doc::Space]),
        }
    }

    // ---- queries ----

    /// Whether the `(` at `i` holds a query.
    fn paren_is_query(&self, i: usize) -> bool {
        let mut i = i;
        // `((SELECT …) UNION (SELECT …))`: the first inside tells.
        for _ in 0..MAX_DEPTH {
            if self.kind_at(i) != Some(TokenKind::OpenParen) || self.close[i] == NONE {
                return false;
            }
            match self.name_free(i + 1) {
                Some("SELECT" | "WITH" | "VALUES") => return true,
                _ if self.kind_at(i + 1) == Some(TokenKind::OpenParen) => i += 1,
                _ => return false,
            }
        }
        false
    }

    fn query(&mut self, r: Range<usize>, sep: Sep) -> Doc<'a> {
        let clauses = self.clauses(r);
        let mut docs = Vec::with_capacity(clauses.len() * 2);
        for clause in clauses {
            if clause.kind == Kind::On && !docs.is_empty() {
                let ending = self.take_ending(&clause);
                let on = self.clause_doc(clause);
                let on = Doc::group(vec![Doc::indent(vec![Doc::Line, on])]);
                docs.push(notes_after(on, ending));
                continue;
            }
            if !docs.is_empty() {
                docs.push(sep.doc());
            }
            docs.push(self.clause(clause));
        }
        Doc::seq(docs)
    }

    fn clauses(&self, r: Range<usize>) -> Vec<Clause> {
        let mut out: Vec<Clause> = Vec::new();
        let mut current: Option<(Kind, Range<usize>)> = None;
        let mut body_start = r.start;
        let mut j = r.start;
        while j < r.end {
            if let Some((kind, head_end)) = self.clause_at(j, r.start) {
                match current.take() {
                    Some((kind, head)) => out.push(Clause { kind, head, body: body_start..j }),
                    None if j > r.start => out.push(Clause {
                        kind: Kind::Lead,
                        head: r.start..r.start,
                        body: r.start..j,
                    }),
                    None => {}
                }
                let head_end = head_end.min(r.end);
                current = Some((kind, j..head_end));
                body_start = head_end;
                j = head_end;
                continue;
            }
            j = self.skip(j, r.end);
        }
        match current {
            Some((kind, head)) => out.push(Clause { kind, head, body: body_start..r.end }),
            None => out.push(Clause { kind: Kind::Lead, head: r.start..r.start, body: r }),
        }
        out
    }

    /// Whether a clause starts at `j` (in a query starting at `start`): its kind and where
    /// its head (its keywords) ends.
    fn clause_at(&self, j: usize, start: usize) -> Option<(Kind, usize)> {
        let word = self.name_free(j)?;
        let prev = if j > start { self.prev_upper(j) } else { None };
        let next = self.name_free(j + 1);
        let next_paren = self.kind_at(j + 1) == Some(TokenKind::OpenParen);
        let after = |words: &[&str]| prev.is_some_and(|p| words.contains(&p));
        let clause = match word {
            "SELECT" => (Kind::List, self.select_head(j)),
            "FROM" => {
                // `a IS [NOT] DISTINCT FROM b` is a comparison.
                let distinct = prev == Some("DISTINCT")
                    && matches!(self.prev_upper(j - 1), Some("IS" | "NOT"));
                if distinct || after(&["DELETE"]) {
                    return None;
                }
                (Kind::List, j + 1)
            }
            "WHERE" | "HAVING" | "QUALIFY" => (Kind::Cond, j + 1),
            "GROUP" | "ORDER" if next == Some("BY") => (Kind::List, j + 2),
            "WINDOW" => (Kind::List, j + 1),
            "LIMIT" | "OFFSET" | "FETCH" => (Kind::Plain, j + 1),
            "FOR" if matches!(next, Some("UPDATE" | "SHARE" | "NO" | "KEY")) => {
                (Kind::Plain, j + 1)
            }
            "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => {
                let all = matches!(next, Some("ALL" | "DISTINCT"));
                (Kind::SetOp, j + 1 + usize::from(all))
            }
            "INSERT"
                if !after(&["THEN", "DO", "ON", "OR", "FOR", "INSTEAD", "BEFORE", "AFTER"]) =>
            {
                let mut end = j + 1;
                if next == Some("OR") {
                    end += 2;
                }
                if self.name_free(end) == Some("INTO") {
                    end += 1;
                }
                (Kind::Plain, end)
            }
            "REPLACE" if next == Some("INTO") && !after(&["THEN", "OR"]) => (Kind::Plain, j + 2),
            "MERGE" => (Kind::Plain, j + 1 + usize::from(next == Some("INTO"))),
            "UPDATE"
                if !after(&[
                    "DO", "THEN", "FOR", "KEY", "OF", "ON", "OR", "INSTEAD", "BEFORE", "AFTER",
                ]) =>
            {
                (Kind::Plain, j + 1 + if next == Some("OR") { 2 } else { 0 })
            }
            "DELETE" if !after(&["THEN", "ON", "OR", "INSTEAD", "BEFORE", "AFTER"]) => {
                (Kind::Plain, j + 1 + usize::from(next == Some("FROM")))
            }
            "SET" => (Kind::List, j + 1),
            "VALUES" if !after(&["DEFAULT"]) => (Kind::List, j + 1),
            "RETURNING" => (Kind::List, j + 1),
            "INTO" if !after(&["INSERT", "REPLACE", "MERGE"]) => (Kind::Plain, j + 1),
            "USING" if !next_paren => (Kind::Plain, j + 1),
            "WHEN" => (Kind::Plain, j + 1),
            "ON" => match next {
                Some("CONFLICT") => (Kind::Plain, j + 2),
                Some("DUPLICATE") => (Kind::List, j + 4),
                _ => (Kind::On, j + 1),
            },
            "WITH" if j == start => (Kind::With, j + 1 + usize::from(next == Some("RECURSIVE"))),
            "JOIN" | "STRAIGHT_JOIN" => (Kind::Plain, j + 1),
            "INNER" | "LEFT" | "RIGHT" | "FULL" | "CROSS" | "NATURAL" | "OUTER" | "SEMI"
            | "ANTI"
                if !next_paren =>
            {
                let mut k = j;
                while k < j + 5
                    && matches!(
                        self.name_free(k),
                        Some(
                            "INNER"
                                | "LEFT"
                                | "RIGHT"
                                | "FULL"
                                | "CROSS"
                                | "NATURAL"
                                | "OUTER"
                                | "SEMI"
                                | "ANTI"
                        )
                    )
                {
                    k += 1;
                }
                if self.name_free(k) != Some("JOIN") {
                    return None;
                }
                (Kind::Plain, k + 1)
            }
            _ => return None,
        };
        Some(clause)
    }

    /// `SELECT [ALL | DISTINCT [ON (…)]]`: where the head ends.
    fn select_head(&self, j: usize) -> usize {
        match self.name_free(j + 1) {
            Some("ALL") => j + 2,
            Some("DISTINCT") => {
                let on = j + 2;
                let paren = on + 1;
                if self.name_free(on) == Some("ON")
                    && self.kind_at(paren) == Some(TokenKind::OpenParen)
                    && self.close[paren] != NONE
                {
                    self.close[paren] + 1
                } else {
                    j + 2
                }
            }
            _ => j + 1,
        }
    }

    fn clause(&mut self, clause: Clause) -> Doc<'a> {
        let ending = self.take_ending(&clause);
        let doc = self.clause_doc(clause);
        notes_after(doc, ending)
    }

    /// The comments after a clause's last token when a line comment is among them: they
    /// end its line after the clause, whose groups don't break for them.
    fn take_ending(&mut self, clause: &Clause) -> Vec<Note<'a>> {
        let last = if clause.body.is_empty() { clause.head.end } else { clause.body.end };
        last.checked_sub(1)
            .filter(|&l| self.sig[l].trailing.last().is_some_and(|n| n.line))
            .map(|l| std::mem::take(&mut self.sig[l].trailing))
            .unwrap_or_default()
    }

    fn clause_doc(&mut self, clause: Clause) -> Doc<'a> {
        let head = self.expr(clause.head.clone());
        let body = clause.body;
        if body.is_empty() {
            return head;
        }
        match clause.kind {
            Kind::Lead => self.expr(body),
            Kind::List => {
                let (items, commas) = self.split_commas(body);
                let many = items.len() > 1;
                let mut inner = vec![Doc::Line];
                inner.extend(self.items(items, &commas));
                let list = vec![Doc::indent(inner)];
                let list = if many && self.options.lists == ListStyle::OnePerLine {
                    Doc::broken(list)
                } else {
                    Doc::group(list)
                };
                Doc::seq(vec![head, list])
            }
            Kind::Cond | Kind::On => Doc::seq(vec![head, Doc::Space, self.cond(body)]),
            Kind::Plain | Kind::SetOp => Doc::seq(vec![head, Doc::Space, self.expr(body)]),
            Kind::With => self.with(head, body),
        }
    }

    /// Items between their commas, each broken onto its line when the group breaks.
    fn items(&mut self, items: Vec<Range<usize>>, commas: &[usize]) -> Vec<Doc<'a>> {
        let mut docs = Vec::with_capacity(items.len() * 3);
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                self.separator(&mut docs, commas[k - 1], Doc::Line);
            }
            docs.push(self.expr(item));
        }
        docs
    }

    /// `WITH a AS (…), b AS (…)`: each CTE's body indented under it.
    fn with(&mut self, head: Doc<'a>, body: Range<usize>) -> Doc<'a> {
        let (items, commas) = self.split_commas(body);
        let mut docs = vec![head, Doc::Space];
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                self.separator(&mut docs, commas[k - 1], Doc::Hard);
            }
            docs.push(self.cte(item));
        }
        Doc::seq(docs)
    }

    fn cte(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut j = r.start;
        let mut as_at = None;
        while j < r.end {
            if self.name_free(j) == Some("AS") {
                as_at = Some(j);
                break;
            }
            j = self.skip(j, r.end);
        }
        let Some(a) = as_at else { return self.expr(r) };
        let mut p = a + 1;
        while matches!(self.name_free(p), Some("NOT" | "MATERIALIZED")) {
            p += 1;
        }
        if p >= r.end || !self.paren_is_query(p) || self.close[p] >= r.end {
            return self.expr(r);
        }
        let close = self.close[p];
        let mut docs = vec![
            self.expr(r.start..p),
            Doc::Space,
            self.tok(p),
            Doc::indent(vec![Doc::Hard, self.query(p + 1..close, Sep::Hard)]),
            Doc::Hard,
            self.tok(close),
        ];
        if close + 1 < r.end {
            docs.extend([Doc::Space, self.expr(close + 1..r.end)]);
        }
        Doc::seq(docs)
    }

    // ---- expressions ----

    /// A condition, broken before its `AND` / `OR` (not a `BETWEEN`'s `AND`).
    fn cond(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut ops = Vec::new();
        let mut between = false;
        let mut j = r.start;
        while j < r.end {
            match self.name_free(j) {
                Some("BETWEEN") => between = true,
                Some("AND") if between => between = false,
                Some("AND" | "OR") => ops.push(j),
                _ => {}
            }
            j = self.skip(j, r.end);
        }
        if ops.is_empty() {
            return self.expr(r);
        }
        let first = self.expr(r.start..ops[0]);
        let mut inner = Vec::with_capacity(ops.len() * 4);
        for (k, &op) in ops.iter().enumerate() {
            let end = ops.get(k + 1).copied().unwrap_or(r.end);
            inner.extend([Doc::Line, self.tok(op), Doc::Space, self.expr(op + 1..end)]);
        }
        Doc::group(vec![first, Doc::indent(inner)])
    }

    /// Tokens on a line, the parentheses and `CASE` blocks among them laid out as groups.
    fn expr(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut docs = Vec::with_capacity(r.len() * 2);
        let mut prev: Option<usize> = None;
        let mut i = r.start;
        while i < r.end {
            if let Some(p) = prev
                && let Some(gap) = self.gap(p, i)
            {
                docs.push(gap);
            }
            let close = self.close[i];
            let case_end = self.end_of[i];
            let last = if self.sig[i].kind == TokenKind::OpenParen && close != NONE && close < r.end
            {
                docs.push(self.paren(i, close));
                close
            } else if case_end != NONE && case_end < r.end {
                docs.push(self.case(i, case_end));
                case_end
            } else {
                docs.push(self.tok(i));
                i
            };
            prev = Some(last);
            i = last + 1;
        }
        if docs.len() == 1 {
            return docs.pop().unwrap_or_else(|| Doc::seq(Vec::new()));
        }
        Doc::seq(docs)
    }

    fn paren(&mut self, open: usize, close: usize) -> Doc<'a> {
        let content = open + 1..close;
        if content.is_empty() {
            return Doc::seq(vec![self.tok(open), self.tok(close)]);
        }
        let inner = if self.paren_is_query(open) {
            self.query(content, Sep::Line)
        } else {
            let (items, commas) = self.split_commas(content.clone());
            if commas.is_empty() {
                self.cond(content)
            } else {
                Doc::seq(self.items(items, &commas))
            }
        };
        Doc::group(vec![
            self.tok(open),
            Doc::indent(vec![Doc::Soft, inner]),
            Doc::Soft,
            self.tok(close),
        ])
    }

    /// `CASE [x] WHEN … THEN … ELSE … END`: on one line if it fits, else each `WHEN` and
    /// the `ELSE` on a line, indented.
    fn case(&mut self, case: usize, end: usize) -> Doc<'a> {
        let mut whens = Vec::new();
        let mut else_at = None;
        let mut j = case + 1;
        while j < end {
            match self.name_free(j) {
                Some("WHEN") if else_at.is_none() => whens.push(j),
                Some("ELSE") => else_at = Some(j),
                _ => {}
            }
            j = self.skip(j, end);
        }
        let Some(&first_when) = whens.first() else {
            let mut docs = vec![self.tok(case)];
            if case + 1 < end {
                docs.extend([Doc::Space, self.expr(case + 1..end)]);
            }
            docs.extend([Doc::Space, self.tok(end)]);
            return Doc::seq(docs);
        };
        let mut docs = vec![self.tok(case)];
        if case + 1 < first_when {
            docs.extend([Doc::Space, self.expr(case + 1..first_when)]);
        }
        let mut inner = Vec::new();
        for (k, &when) in whens.iter().enumerate() {
            let next = whens.get(k + 1).copied().or(else_at).unwrap_or(end);
            let mut then_at = None;
            let mut t = when + 1;
            while t < next {
                if self.name_free(t) == Some("THEN") {
                    then_at = Some(t);
                    break;
                }
                t = self.skip(t, next);
            }
            inner.extend([Doc::Line, self.tok(when)]);
            match then_at {
                Some(then) => {
                    if when + 1 < then {
                        inner.extend([Doc::Space, self.expr(when + 1..then)]);
                    }
                    inner.extend([Doc::Space, self.tok(then)]);
                    if then + 1 < next {
                        inner.extend([Doc::Space, self.expr(then + 1..next)]);
                    }
                }
                None if when + 1 < next => inner.extend([Doc::Space, self.expr(when + 1..next)]),
                None => {}
            }
        }
        if let Some(e) = else_at {
            inner.extend([Doc::Line, self.tok(e)]);
            if e + 1 < end {
                inner.extend([Doc::Space, self.expr(e + 1..end)]);
            }
        }
        docs.extend([Doc::indent(inner), Doc::Line, self.tok(end)]);
        Doc::group(docs)
    }

    /// What goes between tokens `a` and `b` (next to each other) on a line: a space,
    /// nothing, or a line break (two strings a line apart: PostgreSQL joins them only so).
    fn gap(&self, a: usize, b: usize) -> Option<Doc<'a>> {
        use TokenKind as K;
        let (x, y) = (&self.sig[a], &self.sig[b]);
        if x.trailing.last().is_some_and(|n| n.line) {
            // The line comment ended the line already.
            return None;
        }
        let keep = || (!y.glued).then_some(Doc::Space);
        match (x.kind, y.kind) {
            (K::String(_), K::String(_)) if y.newline => Some(Doc::Hard),
            (_, K::Comma | K::Semicolon) => None,
            (K::OpenParen, _) | (_, K::CloseParen) => None,
            (K::Number, K::Dot) | (K::Dot, K::Number) => keep(),
            (K::Dot, _) | (_, K::Dot) => None,
            (K::Cast, _) | (_, K::Cast) => None,
            (_, K::OpenParen) => self.before_paren(a),
            (K::Punct, _) | (_, K::Punct) => keep(),
            (K::Operator, _) if self.unary(a) => keep(),
            _ => Some(Doc::Space),
        }
    }

    /// What goes between the token at `a` and the `(` after it.
    fn before_paren(&self, a: usize) -> Option<Doc<'a>> {
        let x = &self.sig[a];
        let keep = || (!self.sig[a + 1].glued).then_some(Doc::Space);
        match x.kind {
            TokenKind::Word => {
                let upper = self.name_free(a).unwrap_or("");
                if x.keyword && words::call_like(upper) {
                    None
                } else if x.keyword && words::before_paren(upper) {
                    Some(Doc::Space)
                } else {
                    self.spaced_name(a)
                }
            }
            TokenKind::QuotedIdent => self.spaced_name(a),
            TokenKind::Operator if self.unary(a) => keep(),
            TokenKind::Operator | TokenKind::Comma => Some(Doc::Space),
            _ => keep(),
        }
    }

    /// A name and its `(`: a call's arguments right after it, a table's or a CTE's column
    /// list after a space (`INSERT INTO t (a, b)`, `REFERENCES t (id)`, `x (a) AS (…)`).
    fn spaced_name(&self, a: usize) -> Option<Doc<'a>> {
        let mut start = a;
        while start >= 2
            && self.sig[start - 1].kind == TokenKind::Dot
            && self.sig[start - 2].is_name()
        {
            start -= 2;
        }
        let ddl = self.name_free(0) == Some("CREATE");
        let table = match start.checked_sub(1).and_then(|p| self.name_free(p)) {
            Some("INTO" | "TABLE" | "REFERENCES" | "VIEW" | "EXISTS") => true,
            Some("ON") => ddl,
            _ => false,
        };
        // A CTE's columns: `x (a, b) AS (` (not a call's alias: `count(*) AS n`).
        let close = self.close[a + 1];
        let cte = close != NONE
            && self.name_free(close + 1) == Some("AS")
            && (self.kind_at(close + 2) == Some(TokenKind::OpenParen)
                || matches!(self.name_free(close + 2), Some("NOT" | "MATERIALIZED")));
        (table || cte).then_some(Doc::Space)
    }

    /// The operator at `a` stands before its operand alone (`-1`, `+x`, `~mask`).
    fn unary(&self, a: usize) -> bool {
        if !matches!(self.sig[a].text, "-" | "+" | "~") {
            return false;
        }
        let Some(p) = a.checked_sub(1) else { return true };
        let prev = &self.sig[p];
        match prev.kind {
            TokenKind::Operator
            | TokenKind::OpenParen
            | TokenKind::Comma
            | TokenKind::Cast
            | TokenKind::Punct => true,
            TokenKind::Word => prev.keyword,
            _ => false,
        }
    }

    /// The token at `i`, as written (a keyword in the case chosen), with its comments.
    fn tok(&self, i: usize) -> Doc<'a> {
        let s = &self.sig[i];
        let text = if s.keyword {
            match self.options.keyword_case {
                KeywordCase::Upper => cased(s.text, true),
                KeywordCase::Lower => cased(s.text, false),
                KeywordCase::Preserve => Cow::Borrowed(s.text),
            }
        } else {
            Cow::Borrowed(s.text)
        };
        if s.leading.is_empty() && s.trailing.is_empty() {
            return Doc::Text(text);
        }
        let mut docs = Vec::with_capacity(2 + 3 * (s.leading.len() + s.trailing.len()));
        for note in &s.leading {
            docs.extend([Doc::Hard, Doc::text(note.text)]);
        }
        if !s.leading.is_empty() {
            docs.push(Doc::Hard);
        }
        docs.push(Doc::Text(text));
        for note in &s.trailing {
            docs.extend([Doc::Space, Doc::text(note.text)]);
            if note.line {
                docs.push(Doc::Hard);
            }
        }
        Doc::seq(docs)
    }
}

impl Sig<'_> {
    fn is_name(&self) -> bool {
        matches!(self.kind, TokenKind::Word | TokenKind::QuotedIdent)
    }
}

/// `doc` with comments after it on its line.
fn notes_after<'a>(doc: Doc<'a>, notes: Vec<Note<'a>>) -> Doc<'a> {
    if notes.is_empty() {
        return doc;
    }
    let mut docs = vec![doc];
    for note in notes {
        docs.extend([Doc::Space, Doc::text(note.text)]);
        if note.line {
            docs.push(Doc::Hard);
        }
    }
    Doc::seq(docs)
}

/// `text` in upper (or lower) case, borrowed when it already is.
fn cased(text: &str, upper: bool) -> Cow<'_, str> {
    let already = if upper {
        !text.bytes().any(|b| b.is_ascii_lowercase())
    } else {
        !text.bytes().any(|b| b.is_ascii_uppercase())
    };
    if already {
        Cow::Borrowed(text)
    } else if upper {
        Cow::Owned(text.to_ascii_uppercase())
    } else {
        Cow::Owned(text.to_ascii_lowercase())
    }
}

/// The tokens that say something, each with its comments; `None` when the statement is
/// left as written.
fn read<'a>(text: &'a str, tokens: &[Token]) -> Option<Vec<Sig<'a>>> {
    let mut sig: Vec<Sig<'a>> = Vec::with_capacity(tokens.len() / 2 + 1);
    let mut leading: Vec<Note<'a>> = Vec::new();
    // Since the last token or comment kept: a line break, any space.
    let mut newline = false;
    let mut spaced = false;
    // A line break since the last token that says something (its comments trail it until).
    let mut line_since_sig = false;
    for token in tokens {
        let raw = token.text(text);
        match token.kind {
            TokenKind::Whitespace => {
                spaced = true;
                if raw.contains(['\n', '\r']) {
                    newline = true;
                    line_since_sig = true;
                }
            }
            TokenKind::LineComment | TokenKind::BlockComment => {
                let note = Note { text: raw, line: token.kind == TokenKind::LineComment };
                match sig.last_mut() {
                    Some(last) if !line_since_sig => last.trailing.push(note),
                    _ => leading.push(note),
                }
                spaced = true;
                if note.line {
                    line_since_sig = true;
                    newline = true;
                }
            }
            kind => {
                // A psql command (`\set`, `\gexec`) ends at its line: never moved.
                if kind == TokenKind::Punct && raw == "\\" {
                    return None;
                }
                let upper = (kind == TokenKind::Word && raw.len() <= 32)
                    .then(|| raw.to_ascii_uppercase().into_boxed_str());
                sig.push(Sig {
                    kind,
                    text: raw,
                    upper,
                    glued: !spaced && !sig.is_empty(),
                    newline,
                    leading: std::mem::take(&mut leading),
                    trailing: Vec::new(),
                    keyword: false,
                });
                newline = false;
                spaced = false;
                line_since_sig = false;
            }
        }
    }
    // Comments after the last token belong to it (none: a statement ends at a token).
    if !leading.is_empty() {
        let last = sig.last_mut()?;
        last.trailing.extend(leading);
    }
    Some(sig)
}
