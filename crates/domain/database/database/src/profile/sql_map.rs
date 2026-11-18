//! A profile's nodes taken back to the statement's text ([`SqlMap`], pure, on the 39a
//! tokens): the tables named in its `FROM` / `JOIN` (`UPDATE`, `INTO`) with their aliases,
//! where a node's table is, where a condition's column is compared in a `WHERE` / `ON` /
//! `HAVING` (the whole comparison), and the clauses the rules read (the `WHERE` of the top
//! level, `ORDER BY`, `LIMIT`, `OFFSET`, `SELECT *`). A place is given only when it's the only
//! one: two `orders` without aliases, or a column compared twice, give none.
//!
//! Places are in the text read (the statement as sent, its values written in); a finding's
//! place goes to the document through the statement's [`crate::sql::BindMap`].

use std::ops::Range;

use crate::TableRef;
use crate::profile::node::ObjectRef;
use crate::sql::lex::{Token, TokenKind, lex};
use crate::sql::tables::{CLAUSES, name_at};
use crate::sql::{Flavor, identifier, significant};

/// A table named in the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableOccurrence {
    pub table: TableRef,
    pub alias: Option<String>,
    /// Its name (and alias) in the text.
    pub range: Range<usize>,
    /// Its depth in parentheses (`0`: the statement's own level).
    pub depth: usize,
}

impl TableOccurrence {
    /// It's named `name` (its alias, else its table's name; without case).
    pub fn answers_to(&self, name: &str) -> bool {
        match &self.alias {
            Some(alias) => alias.eq_ignore_ascii_case(name),
            None => self.table.name.eq_ignore_ascii_case(name),
        }
    }
}

/// What a clause is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClauseKind {
    Select,
    From,
    Where,
    On,
    Having,
    GroupBy,
    OrderBy,
    Limit,
    Offset,
    Fetch,
}

/// A clause: its keyword's place and its content's (token indices among the significant
/// ones, and its depth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub kind: ClauseKind,
    pub keyword: Range<usize>,
    /// Its content's text (after the keyword).
    pub range: Range<usize>,
    pub depth: usize,
    /// The group of parentheses it's in (clauses of one `SELECT` share it).
    pub group: usize,
    tokens: Range<usize>,
}

/// A statement's tables and clauses, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlMap {
    text: String,
    sig: Vec<(Token, usize)>,
    tables: Vec<TableOccurrence>,
    clauses: Vec<Clause>,
}

/// The words that end a clause (at its depth).
const BOUNDARIES: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "WINDOW",
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
    "ON",
    "USING",
    "RETURNING",
    "FOR",
    "SET",
    "VALUES",
];

impl SqlMap {
    pub fn new(text: &str, flavor: Flavor) -> Self {
        let tokens = lex(text, flavor);
        let sig = significant(&tokens);
        let tables = tables(text, &sig, flavor);
        let clauses = clauses(text, &sig);
        Self { text: text.to_owned(), sig, tables, clauses }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn tables(&self) -> &[TableOccurrence] {
        &self.tables
    }

    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    /// The clauses of `kind` at the statement's own level.
    pub fn top(&self, kind: ClauseKind) -> Option<&Clause> {
        self.clauses.iter().find(|c| c.kind == kind && c.depth == 0)
    }

    /// The table `name` (an alias, or a table's name) means, when only one does.
    pub fn resolve(&self, name: &str) -> Option<&TableOccurrence> {
        let by_alias: Vec<&TableOccurrence> = self
            .tables
            .iter()
            .filter(|t| t.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name)))
            .collect();
        let found = if by_alias.is_empty() {
            self.tables.iter().filter(|t| t.table.name.eq_ignore_ascii_case(name)).collect()
        } else {
            by_alias
        };
        let first = *found.first()?;
        found.iter().all(|t| t.table == first.table).then_some(first)
    }

    /// Where `object` (a node's table, with its alias) is named, when it's named once.
    pub fn table_range(&self, object: &ObjectRef) -> Option<Range<usize>> {
        let alias = object.alias.as_deref().filter(|a| !a.eq_ignore_ascii_case(&object.name));
        let found: Vec<&TableOccurrence> = self
            .tables
            .iter()
            .filter(|t| {
                t.table.name.eq_ignore_ascii_case(&object.name)
                    && match alias {
                        Some(a) => t.alias.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a)),
                        None => true,
                    }
            })
            .collect();
        match found.as_slice() {
            [one] => Some(one.range.clone()),
            _ => None,
        }
    }

    /// Where `column` is compared in a `WHERE`, `ON` or `HAVING` (the whole comparison:
    /// `o.status = 'paid'`), qualified by one of `qualifiers` or not at all — when it's
    /// compared once.
    pub fn predicate_range(&self, column: &str, qualifiers: &[&str]) -> Option<Range<usize>> {
        let mut found: Vec<Range<usize>> = Vec::new();
        for clause in &self.clauses {
            if !matches!(clause.kind, ClauseKind::Where | ClauseKind::On | ClauseKind::Having) {
                continue;
            }
            for k in clause.tokens.clone() {
                let (token, depth) = self.sig[k];
                if !token.is_name() || !identifier(&self.text, &token).eq_ignore_ascii_case(column)
                {
                    continue;
                }
                let next = self.sig.get(k + 1).map(|(t, _)| t.kind);
                if matches!(next, Some(TokenKind::Dot | TokenKind::OpenParen)) {
                    continue;
                }
                if k > 0 && self.sig[k - 1].0.kind == TokenKind::Dot {
                    let Some((q, _)) = k.checked_sub(2).map(|q| self.sig[q]) else { continue };
                    let q = identifier(&self.text, &q);
                    if !qualifiers.iter().any(|x| x.eq_ignore_ascii_case(&q)) {
                        continue;
                    }
                }
                let range = self.comparison(k, depth, clause.tokens.clone());
                if !found.contains(&range) {
                    found.push(range);
                }
            }
        }
        match found.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        }
    }

    /// The comparison around `sig[k]`, within `bounds`: from the `AND` / `OR` / `(` before it
    /// to the one after it.
    fn comparison(&self, k: usize, depth: usize, bounds: Range<usize>) -> Range<usize> {
        let is_bool = |i: usize| {
            let t = self.sig[i].0;
            ["AND", "OR", "NOT"].iter().any(|w| t.is_word(&self.text, w))
        };
        // Out of the calls and parentheses around the column alone (`lower(email)`,
        // `(code)::text`): the comparison is theirs.
        let (mut lo, mut hi0, mut depth) = (k, k + 1, depth);
        while lo > bounds.start && depth > 0 {
            let open = lo - 1;
            let (o, od) = self.sig[open];
            if o.kind != TokenKind::OpenParen || od + 1 != depth {
                break;
            }
            let Some(close) = (hi0..bounds.end).find(|&j| {
                self.sig[j].0.kind == TokenKind::CloseParen && self.sig[j].1 + 1 == depth
            }) else {
                break;
            };
            let call = open > bounds.start
                && self.sig[open - 1].0.is_name()
                && ![
                    "AND", "OR", "NOT", "WHERE", "ON", "HAVING", "IN", "ANY", "ALL", "EXISTS",
                    "SELECT",
                ]
                .iter()
                .any(|w| self.sig[open - 1].0.is_word(&self.text, w));
            if !call && close != hi0 {
                break;
            }
            lo = if call { open - 1 } else { open };
            hi0 = close + 1;
            depth -= 1;
        }
        while lo > bounds.start {
            let (_, d) = self.sig[lo - 1];
            // An `AND` / `OR` at its depth, or the parenthesis it's in.
            if d < depth || (d == depth && is_bool(lo - 1)) {
                break;
            }
            lo -= 1;
        }
        let mut hi = hi0;
        let mut between = false;
        while hi < bounds.end {
            let (t, d) = self.sig[hi];
            if d < depth {
                break;
            }
            if d == depth {
                if t.is_word(&self.text, "BETWEEN") {
                    between = true;
                } else if t.is_word(&self.text, "AND") && between {
                    between = false;
                } else if is_bool(hi) {
                    break;
                }
            }
            hi += 1;
        }
        self.sig[lo].0.start..self.sig[hi - 1].0.end
    }

    /// The tokens of a clause, as text slices with their kinds (for the rules that read
    /// `ORDER BY` or a number).
    pub fn clause_tokens(&self, clause: &Clause) -> Vec<(TokenKind, &str, usize)> {
        clause
            .tokens
            .clone()
            .map(|i| (self.sig[i].0.kind, self.sig[i].0.text(&self.text), self.sig[i].1))
            .collect()
    }

    /// The columns of the top level's `ORDER BY` (or `GROUP BY`), each with whether it's
    /// descending; `None` when one is an expression, a number or a qualified name of
    /// another table than `qualifiers` say.
    pub fn sort_columns(
        &self,
        kind: ClauseKind,
        qualifiers: &[&str],
    ) -> Option<Vec<(String, bool)>> {
        let clause = self.top(kind)?;
        let tokens = &self.sig[clause.tokens.clone()];
        let mut out = Vec::new();
        for item in tokens.split(|(t, d)| t.kind == TokenKind::Comma && *d == clause.depth) {
            let mut words: Vec<&Token> = item.iter().map(|(t, _)| t).collect();
            let mut desc = false;
            // `NULLS FIRST` / `LAST`, `ASC` / `DESC`, `COLLATE x` at the end.
            while let Some(last) = words.last() {
                let w = last.text(&self.text).to_ascii_uppercase();
                match w.as_str() {
                    "ASC" => {
                        words.pop();
                    }
                    "DESC" => {
                        desc = true;
                        words.pop();
                    }
                    "FIRST" | "LAST" if words.len() >= 2 => {
                        words.pop();
                        words.pop();
                    }
                    _ => break,
                }
            }
            let name = match words.as_slice() {
                [one] if one.is_name() => identifier(&self.text, one).into_owned(),
                [q, dot, one] if q.is_name() && dot.kind == TokenKind::Dot && one.is_name() => {
                    let q = identifier(&self.text, q);
                    if !qualifiers.iter().any(|x| x.eq_ignore_ascii_case(&q)) {
                        return None;
                    }
                    identifier(&self.text, one).into_owned()
                }
                _ => return None,
            };
            out.push((name, desc));
        }
        (!out.is_empty()).then_some(out)
    }

    /// `*` (or `t.*`) in the top level's select list: its place.
    pub fn select_star(&self) -> Option<Range<usize>> {
        let clause = self.top(ClauseKind::Select)?;
        clause.tokens.clone().find_map(|i| {
            let (t, d) = self.sig[i];
            let star = t.kind == TokenKind::Operator && t.text(&self.text) == "*";
            let alone = i == clause.tokens.start
                || matches!(self.sig[i - 1].0.kind, TokenKind::Comma | TokenKind::Dot)
                || self.sig[i - 1].0.is_word(&self.text, "DISTINCT")
                || self.sig[i - 1].0.is_word(&self.text, "ALL");
            (star && d == clause.depth && alone).then(|| t.range())
        })
    }

    /// A `LIMIT` (or `FETCH FIRST`) whose `SELECT` has no `ORDER BY`: the keyword's place.
    pub fn limit_without_order(&self) -> Option<Range<usize>> {
        self.clauses.iter().find_map(|c| {
            if !matches!(c.kind, ClauseKind::Limit | ClauseKind::Fetch) {
                return None;
            }
            let ordered = self
                .clauses
                .iter()
                .any(|o| o.kind == ClauseKind::OrderBy && o.group == c.group && o.depth == c.depth);
            (!ordered).then(|| c.keyword.clone())
        })
    }

    /// The top level's `OFFSET` (or the first of MySQL's and SQLite's `LIMIT a, b`), when a
    /// number: it and its place.
    pub fn offset(&self) -> Option<(u64, Range<usize>)> {
        let number = |i: usize| {
            let (t, _) = self.sig.get(i)?;
            (t.kind == TokenKind::Number)
                .then(|| t.text(&self.text).replace('_', "").parse::<u64>().ok())
                .flatten()
                .map(|n| (n, t.range()))
        };
        if let Some(c) = self.top(ClauseKind::Offset) {
            return number(c.tokens.start).map(|(n, r)| (n, c.keyword.start..r.end));
        }
        let c = self.top(ClauseKind::Limit)?;
        let comma = c.tokens.clone().find(|&i| self.sig[i].0.kind == TokenKind::Comma)?;
        let (n, r) = number(comma.checked_sub(1)?)?;
        Some((n, c.keyword.start..r.end))
    }
}

/// The tables after `FROM`, `JOIN`, `UPDATE` and `INTO` (several after one `FROM`, with
/// commas), each with its alias.
fn tables(text: &str, sig: &[(Token, usize)], flavor: Flavor) -> Vec<TableOccurrence> {
    let mut out = Vec::new();
    let word = |i: usize, w: &str| sig.get(i).is_some_and(|(t, _)| t.is_word(text, w));
    for i in 0..sig.len() {
        let (t, depth) = sig[i];
        let after = ["FROM", "JOIN", "UPDATE", "INTO"].iter().any(|w| t.is_word(text, w));
        if !after {
            continue;
        }
        let mut at = i + 1;
        while word(at, "ONLY") || word(at, "LATERAL") {
            at += 1;
        }
        while let Some((table, next)) = name_at(text, sig, at, flavor) {
            // A function (`generate_series(…)`), not a table.
            if sig.get(next).is_some_and(|(t, _)| t.kind == TokenKind::OpenParen) {
                break;
            }
            let start = sig[at].0.start;
            let mut end = sig[next - 1].0.end;
            let mut j = next;
            let mut alias = None;
            if word(j, "AS") && sig.get(j + 1).is_some_and(|(t, _)| t.is_name()) {
                alias = Some(identifier(text, &sig[j + 1].0).into_owned());
                end = sig[j + 1].0.end;
                j += 2;
            } else if let Some((a, _)) = sig.get(j)
                && (a.kind == TokenKind::QuotedIdent
                    || (a.kind == TokenKind::Word
                        && !CLAUSES.iter().any(|c| a.is_word(text, c))
                        && !BOUNDARIES.iter().any(|c| a.is_word(text, c))
                        && !a.is_word(text, "DEFAULT")))
            {
                alias = Some(identifier(text, a).into_owned());
                end = a.end;
                j += 1;
            }
            out.push(TableOccurrence { table, alias, range: start..end, depth });
            // `FROM a, b`.
            if t.is_word(text, "FROM")
                && sig.get(j).is_some_and(|(c, _)| c.kind == TokenKind::Comma)
            {
                at = j + 1;
                continue;
            }
            break;
        }
    }
    out
}

/// The clauses, each to the next word ending it at its depth (or its parentheses' end).
fn clauses(text: &str, sig: &[(Token, usize)]) -> Vec<Clause> {
    // The group of parentheses each token is in.
    let mut groups = Vec::with_capacity(sig.len());
    let mut stack = vec![0usize];
    let mut next_group = 1;
    for (t, _) in sig {
        match t.kind {
            TokenKind::OpenParen => {
                groups.push(*stack.last().unwrap_or(&0));
                stack.push(next_group);
                next_group += 1;
            }
            TokenKind::CloseParen => {
                if stack.len() > 1 {
                    stack.pop();
                }
                groups.push(*stack.last().unwrap_or(&0));
            }
            _ => groups.push(*stack.last().unwrap_or(&0)),
        }
    }
    let word = |i: usize, w: &str| sig.get(i).is_some_and(|(t, _)| t.is_word(text, w));
    let mut out = Vec::new();
    for i in 0..sig.len() {
        let (t, depth) = sig[i];
        if t.kind != TokenKind::Word {
            continue;
        }
        let w = t.text(text).to_ascii_uppercase();
        let (kind, skip) = match w.as_str() {
            "SELECT" => (ClauseKind::Select, 1),
            "FROM" => (ClauseKind::From, 1),
            "WHERE" => (ClauseKind::Where, 1),
            "ON" => (ClauseKind::On, 1),
            "HAVING" => (ClauseKind::Having, 1),
            "GROUP" if word(i + 1, "BY") => (ClauseKind::GroupBy, 2),
            "ORDER" if word(i + 1, "BY") => (ClauseKind::OrderBy, 2),
            "LIMIT" => (ClauseKind::Limit, 1),
            "OFFSET" => (ClauseKind::Offset, 1),
            "FETCH" if word(i + 1, "FIRST") || word(i + 1, "NEXT") => (ClauseKind::Fetch, 1),
            _ => continue,
        };
        // A window's `ORDER BY` (`OVER (ORDER BY …)`) isn't the query's.
        if kind == ClauseKind::OrderBy
            && i > 0
            && sig[i - 1].0.kind == TokenKind::OpenParen
            && i >= 2
            && word(i - 2, "OVER")
        {
            continue;
        }
        let start = i + skip;
        let mut end = start;
        while end < sig.len() {
            let (e, d) = sig[end];
            if d < depth || e.kind == TokenKind::Semicolon {
                break;
            }
            if d == depth
                && e.kind == TokenKind::Word
                && BOUNDARIES.iter().any(|b| e.is_word(text, b))
            {
                break;
            }
            end += 1;
        }
        let keyword = t.start..sig[start.min(sig.len()) - 1].0.end;
        let range = if start < end { sig[start].0.start..sig[end - 1].0.end } else { 0..0 };
        out.push(Clause { kind, keyword, range, depth, group: groups[i], tokens: start..end });
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/sql_map.rs"]
mod tests;
