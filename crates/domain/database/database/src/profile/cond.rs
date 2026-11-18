//! A step's condition read as the rules need it (pure, with the 39a lexer, by the engine's
//! flavor): the columns compared with a value or a parameter (equality, a range, `IN` and
//! `= ANY (…)`, `IS NULL`, `LIKE`), a column wrapped in a function (`lower(email)`) or cast
//! (`(status)::text`), the `OR`s at its top level, a `LIKE` starting with `%`, and the pairs of
//! columns a join compares. PostgreSQL's forms (`((status)::text = 'x'::text)`,
//! `(id = ANY ('{1,2}'::integer[]))`, `~~` for `LIKE`) and SQLite's (`a=? AND b>?`) read the
//! same; anything else (an expression, a subplan's output) is only a column mentioned.
//!
//! A column belongs to a node's table only when it's unqualified or qualified by the node's
//! alias or name ([`ColumnRef::of_object`]).

use std::ops::Range;

use crate::profile::node::ObjectRef;
use crate::sql::lex::{Token, TokenKind, lex};
use crate::sql::{Flavor, identifier};

/// A column named in a condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    /// The table or alias it's qualified by (`o` of `o.customer_id`).
    pub qualifier: Option<String>,
    pub name: String,
    /// Where it is in the condition's text.
    pub range: Range<usize>,
}

impl ColumnRef {
    /// It is `object`'s: unqualified, or qualified by its alias or its name.
    pub fn of_object(&self, object: &ObjectRef) -> bool {
        match &self.qualifier {
            None => true,
            Some(q) => {
                object.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(q))
                    || object.name.eq_ignore_ascii_case(q)
            }
        }
    }

    /// Its name compared as the engines compare unquoted names (without case).
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// How a column is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compare {
    /// `=`, `==`.
    Eq,
    /// `IN (…)`, `= ANY (…)`.
    In,
    /// `<`, `<=`, `>`, `>=`, `BETWEEN`.
    Range,
    /// `IS NULL`, `IS NOT NULL`.
    IsNull,
    /// `LIKE`, `ILIKE`, `~~`, `~~*`, `GLOB`.
    Like,
    /// `<>`, `!=`, `NOT LIKE`, `NOT IN`… (an index rarely helps).
    Not,
}

impl Compare {
    /// An index on the column can find the rows (`=`, `IN`, `IS NULL`).
    pub fn is_equality(self) -> bool {
        matches!(self, Compare::Eq | Compare::In | Compare::IsNull)
    }
}

/// The function a column is wrapped in (`lower(email)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapper {
    /// As written (`lower`, `pg_catalog.lower`).
    pub function: String,
    /// The call's text in the condition.
    pub range: Range<usize>,
    /// The column is its only argument.
    pub single: bool,
}

/// A column compared with a value (or a parameter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    pub column: ColumnRef,
    pub compare: Compare,
    pub wrapper: Option<Wrapper>,
    /// The type the column is cast to (`text` of `(status)::text`).
    pub cast: Option<String>,
    /// The value is a literal (a string, a number, `NULL`, `TRUE`), not a parameter or an
    /// expression.
    pub literal: bool,
    /// `LIKE` with a pattern starting with `%`.
    pub leading_wildcard: bool,
    /// Inside an `OR` (it alone doesn't decide which rows are kept).
    pub in_or: bool,
    /// The whole comparison's text in the condition.
    pub range: Range<usize>,
}

impl Predicate {
    /// The column alone, neither wrapped nor cast.
    pub fn is_plain(&self) -> bool {
        self.wrapper.is_none() && self.cast.is_none()
    }
}

/// Two columns compared (a join's condition: `o.customer_id = c.id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinPair {
    pub left: ColumnRef,
    pub right: ColumnRef,
    pub range: Range<usize>,
}

impl JoinPair {
    /// The pair's column of `object` and the other one (`None`: neither is its, or both
    /// could be).
    pub fn side_of(&self, object: &ObjectRef) -> Option<(&ColumnRef, &ColumnRef)> {
        match (self.left.of_object(object), self.right.of_object(object)) {
            (true, false) => Some((&self.left, &self.right)),
            (false, true) => Some((&self.right, &self.left)),
            _ => None,
        }
    }
}

/// A condition, read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedCondition {
    pub predicates: Vec<Predicate>,
    pub joins: Vec<JoinPair>,
    /// The columns of each branch of an `OR` at the top level (empty: no such `OR`).
    pub or_branches: Vec<Vec<ColumnRef>>,
    /// Every column mentioned, in order.
    pub columns: Vec<ColumnRef>,
}

impl ParsedCondition {
    /// The predicates on `object`'s columns that decide the rows kept (not inside an `OR`).
    pub fn of_object<'a>(&'a self, object: &'a ObjectRef) -> impl Iterator<Item = &'a Predicate> {
        self.predicates.iter().filter(move |p| !p.in_or && p.column.of_object(object))
    }
}

/// `text` (a condition, or a statement's `WHERE` clause without its keyword) read by
/// `flavor`'s rules.
pub fn parse(text: &str, flavor: Flavor) -> ParsedCondition {
    let tokens: Vec<Token> = lex(text, flavor).into_iter().filter(|t| !t.is_trivia()).collect();
    let mut out = ParsedCondition::default();
    let reader = Reader { text, tokens: &tokens };
    reader.expression(0, tokens.len(), false, true, &mut out);
    out
}

struct Reader<'a> {
    text: &'a str,
    tokens: &'a [Token],
}

/// Words that are values, not columns.
const VALUE_WORDS: &[&str] = &[
    "NULL",
    "TRUE",
    "FALSE",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "CURRENT_USER",
    "SESSION_USER",
    "USER",
    "DEFAULT",
];

/// What a side of a comparison is.
#[derive(Debug, Clone)]
enum Operand {
    Column { column: ColumnRef, wrapper: Option<Wrapper>, cast: Option<String> },
    Value { literal: bool, leading_wildcard: bool },
    Other,
}

impl Reader<'_> {
    fn word(&self, at: usize, word: &str) -> bool {
        self.tokens.get(at).is_some_and(|t| t.is_word(self.text, word))
    }

    fn kind(&self, at: usize) -> Option<TokenKind> {
        self.tokens.get(at).map(|t| t.kind)
    }

    fn range(&self, lo: usize, hi: usize) -> Range<usize> {
        if lo >= hi {
            return 0..0;
        }
        self.tokens[lo].start..self.tokens[hi - 1].end
    }

    /// The index of the parenthesis closing the one at `open` (before `hi`).
    fn closing(&self, open: usize, hi: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in open..hi {
            match self.tokens[i].kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// `lo..hi` without the parentheses around all of it.
    fn strip(&self, mut lo: usize, mut hi: usize) -> (usize, usize) {
        while hi > lo + 1
            && self.kind(lo) == Some(TokenKind::OpenParen)
            && self.closing(lo, hi) == Some(hi - 1)
        {
            lo += 1;
            hi -= 1;
        }
        (lo, hi)
    }

    /// The places of `words` at the top level of `lo..hi` (an `AND` of a `BETWEEN` skipped).
    fn split(&self, lo: usize, hi: usize, word: &str) -> Vec<(usize, usize)> {
        let mut parts = Vec::new();
        let mut depth = 0usize;
        let mut start = lo;
        let mut between = false;
        for i in lo..hi {
            match self.tokens[i].kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth = depth.saturating_sub(1),
                _ if depth == 0 => {
                    if self.word(i, "BETWEEN") {
                        between = true;
                    } else if self.word(i, word) {
                        if word.eq_ignore_ascii_case("AND") && between {
                            between = false;
                        } else {
                            parts.push((start, i));
                            start = i + 1;
                        }
                    }
                }
                _ => {}
            }
        }
        parts.push((start, hi));
        parts
    }

    fn expression(&self, lo: usize, hi: usize, in_or: bool, top: bool, out: &mut ParsedCondition) {
        let (lo, hi) = self.strip(lo, hi);
        if lo >= hi {
            return;
        }
        let ors = self.split(lo, hi, "OR");
        if ors.len() > 1 {
            for &(a, b) in &ors {
                let before = out.columns.len();
                self.expression(a, b, true, false, out);
                if top {
                    out.or_branches.push(out.columns[before..].to_vec());
                }
            }
            return;
        }
        let ands = self.split(lo, hi, "AND");
        if ands.len() > 1 {
            for &(a, b) in &ands {
                // An AND's parts at the top keep the top (`a AND (b OR c)`'s OR is the top's).
                self.expression(a, b, in_or, top, out);
            }
            return;
        }
        let mut lo = lo;
        let mut negated = false;
        while self.word(lo, "NOT") {
            negated = true;
            lo += 1;
        }
        if lo < hi && self.kind(lo) == Some(TokenKind::OpenParen) {
            let (a, b) = self.strip(lo, hi);
            if (a, b) != (lo, hi) {
                // `NOT (…)`: what's inside never selects rows by itself.
                self.expression(a, b, true, false, out);
                return;
            }
        }
        self.predicate(lo, hi, in_or || negated, negated, out);
    }

    /// Where the comparison's operator is: its place, its length in tokens, what it is.
    fn operator(&self, lo: usize, hi: usize) -> Option<(usize, usize, Compare)> {
        let mut depth = 0usize;
        for i in lo..hi {
            let t = self.tokens[i];
            match t.kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth = depth.saturating_sub(1),
                TokenKind::Operator if depth == 0 => {
                    let op = t.text(self.text);
                    let compare = match op {
                        "=" | "==" => {
                            if self.word(i + 1, "ANY") || self.word(i + 1, "SOME") {
                                return Some((i, 2, Compare::In));
                            }
                            Compare::Eq
                        }
                        "<" | "<=" | ">" | ">=" => Compare::Range,
                        "<>" | "!=" => Compare::Not,
                        "~~" | "~~*" => Compare::Like,
                        "!~~" | "!~~*" | "~" | "~*" | "!~" | "!~*" => Compare::Not,
                        _ => continue,
                    };
                    return Some((i, 1, compare));
                }
                TokenKind::Word if depth == 0 && i > lo => {
                    let w = t.text(self.text).to_ascii_uppercase();
                    let not = self.word(i - 1, "NOT");
                    let compare = match w.as_str() {
                        "LIKE" | "ILIKE" | "GLOB" => {
                            if not {
                                Compare::Not
                            } else {
                                Compare::Like
                            }
                        }
                        "IN" => {
                            if not {
                                Compare::Not
                            } else {
                                Compare::In
                            }
                        }
                        "BETWEEN" => {
                            if not {
                                Compare::Not
                            } else {
                                Compare::Range
                            }
                        }
                        "IS" => {
                            // `IS [NOT] NULL`; `IS [NOT] DISTINCT FROM`, `IS TRUE` aren't searches.
                            let at = if self.word(i + 1, "NOT") { i + 2 } else { i + 1 };
                            if self.word(at, "NULL") {
                                return Some((i, at + 1 - i, Compare::IsNull));
                            }
                            return Some((i, at - i, Compare::Not));
                        }
                        _ => continue,
                    };
                    let (place, len) = if not { (i - 1, 2) } else { (i, 1) };
                    return Some((place, len, compare));
                }
                _ => {}
            }
        }
        None
    }

    fn predicate(
        &self,
        lo: usize,
        hi: usize,
        in_or: bool,
        negated: bool,
        out: &mut ParsedCondition,
    ) {
        let Some((at, len, compare)) = self.operator(lo, hi) else {
            self.mention(lo, hi, out);
            return;
        };
        let left = self.operand(lo, at, out);
        // After the operator (`= ANY`: its list is the value; `IS NULL` is its own value;
        // `BETWEEN a AND b`: both bounds).
        let right = if compare == Compare::IsNull {
            Operand::Value { literal: true, leading_wildcard: false }
        } else if self.word(at + len - 1, "BETWEEN") {
            let bounds = self.split(at + len, hi, "AND");
            let values: Vec<Operand> = match bounds.as_slice() {
                [(a, b)] => {
                    // `split` skips the AND right after a BETWEEN: find it by hand.
                    match (*a..*b).find(|&i| self.word(i, "AND")) {
                        Some(and) => {
                            vec![self.operand(*a, and, out), self.operand(and + 1, *b, out)]
                        }
                        None => vec![Operand::Other],
                    }
                }
                many => many.iter().map(|&(a, b)| self.operand(a, b, out)).collect(),
            };
            if values.iter().all(|v| matches!(v, Operand::Value { .. })) {
                let literal =
                    values.iter().all(|v| matches!(v, Operand::Value { literal: true, .. }));
                Operand::Value { literal, leading_wildcard: false }
            } else {
                Operand::Other
            }
        } else {
            self.operand(at + len, hi, out)
        };
        let compare = if negated { Compare::Not } else { compare };
        let range = self.range(lo, hi);
        match (left, right) {
            (
                Operand::Column { column, wrapper, cast },
                Operand::Value { literal, leading_wildcard },
            )
            | (
                Operand::Value { literal, leading_wildcard },
                Operand::Column { column, wrapper, cast },
            ) => {
                out.predicates.push(Predicate {
                    column,
                    compare,
                    wrapper,
                    cast,
                    literal,
                    leading_wildcard: compare == Compare::Like && leading_wildcard,
                    in_or,
                    range,
                });
            }
            (
                Operand::Column { column: left, wrapper: None, .. },
                Operand::Column { column: right, wrapper: None, .. },
            ) if compare == Compare::Eq => {
                out.joins.push(JoinPair { left, right, range });
            }
            _ => {}
        }
    }

    /// Every column of `lo..hi`, mentioned.
    fn mention(&self, lo: usize, hi: usize, out: &mut ParsedCondition) {
        let mut i = lo;
        while i < hi {
            if let Some((column, next)) = self.column_at(i, hi) {
                out.columns.push(column);
                i = next;
            } else {
                i += 1;
            }
        }
    }

    /// A column name at `at` (`name`, `q.name`, `s.q.name`; not a function, a value word
    /// or a type after `::`): it and the index after it.
    fn column_at(&self, at: usize, hi: usize) -> Option<(ColumnRef, usize)> {
        let t = *self.tokens.get(at)?;
        if !t.is_name() || at >= hi {
            return None;
        }
        if at > 0 && matches!(self.kind(at - 1), Some(TokenKind::Cast | TokenKind::Dot)) {
            return None;
        }
        if t.kind == TokenKind::Word {
            let w = t.text(self.text);
            if VALUE_WORDS.iter().any(|v| v.eq_ignore_ascii_case(w)) || is_keyword(w) {
                return None;
            }
        }
        let mut parts = vec![(identifier(self.text, &t).into_owned(), t.range())];
        let mut i = at + 1;
        while i + 1 < hi && self.kind(i) == Some(TokenKind::Dot) && self.tokens[i + 1].is_name() {
            let n = self.tokens[i + 1];
            parts.push((identifier(self.text, &n).into_owned(), n.range()));
            i += 2;
        }
        // A function's name, not a column.
        if self.kind(i) == Some(TokenKind::OpenParen) && i < hi {
            return None;
        }
        let (name, last) = parts.pop()?;
        let qualifier = parts.pop().map(|(q, _)| q);
        let start = if qualifier.is_some() || !parts.is_empty() {
            self.tokens[at].start
        } else {
            last.start
        };
        Some((ColumnRef { qualifier, name, range: start..last.end }, i))
    }

    fn operand(&self, lo: usize, hi: usize, out: &mut ParsedCondition) -> Operand {
        // A list of values (`IN (1, 2)`): commas inside its parentheses.
        if self.kind(lo) == Some(TokenKind::OpenParen) && self.closing(lo, hi) == Some(hi - 1) {
            let items = self.args(lo + 1, hi - 1);
            if items.len() > 1 {
                let values: Vec<Operand> =
                    items.iter().map(|&(a, b)| self.operand(a, b, out)).collect();
                if values.iter().all(|v| matches!(v, Operand::Value { .. })) {
                    let literal =
                        values.iter().all(|v| matches!(v, Operand::Value { literal: true, .. }));
                    return Operand::Value { literal, leading_wildcard: false };
                }
                return Operand::Other;
            }
        }
        let (lo, hi) = self.strip(lo, hi);
        if lo >= hi {
            return Operand::Other;
        }
        // `x::type` (the last cast at the top level).
        let mut depth = 0usize;
        let mut cast_at = None;
        for i in lo..hi {
            match self.tokens[i].kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth = depth.saturating_sub(1),
                TokenKind::Cast if depth == 0 => cast_at = Some(i),
                _ => {}
            }
        }
        if let Some(c) = cast_at {
            let ty = self.text[self.range(c + 1, hi)].trim().to_ascii_lowercase();
            return match self.operand(lo, c, out) {
                Operand::Column { column, wrapper, cast } => {
                    Operand::Column { column, wrapper, cast: cast.or(Some(ty)) }
                }
                other => other,
            };
        }
        let first = self.tokens[lo];
        // A value.
        match first.kind {
            TokenKind::String(_) | TokenKind::DollarBody if hi == lo + 1 => {
                let raw = first.text(self.text);
                let body = raw.trim_start_matches(['E', 'e', 'N', 'n']).trim_start_matches('\'');
                return Operand::Value { literal: true, leading_wildcard: body.starts_with('%') };
            }
            TokenKind::Number if hi == lo + 1 => {
                return Operand::Value { literal: true, leading_wildcard: false };
            }
            TokenKind::Parameter(_) if hi == lo + 1 => {
                return Operand::Value { literal: false, leading_wildcard: false };
            }
            TokenKind::Operator
                if first.text(self.text) == "-"
                    && hi == lo + 2
                    && self.kind(lo + 1) == Some(TokenKind::Number) =>
            {
                return Operand::Value { literal: true, leading_wildcard: false };
            }
            _ => {}
        }
        // `CAST(x AS type)`.
        if self.word(lo, "CAST")
            && self.kind(lo + 1) == Some(TokenKind::OpenParen)
            && let Some(close) = self.closing(lo + 1, hi)
            && close == hi - 1
        {
            {
                let inner = lo + 2..close;
                let as_at = (inner.clone()).rev().find(|&i| self.word(i, "AS"));
                if let Some(as_at) = as_at {
                    let ty = self.text[self.range(as_at + 1, close)].trim().to_ascii_lowercase();
                    return match self.operand(inner.start, as_at, out) {
                        Operand::Column { column, wrapper, cast } => {
                            Operand::Column { column, wrapper, cast: cast.or(Some(ty)) }
                        }
                        other => other,
                    };
                }
            }
        }
        // A typed literal (`DATE '2024-01-01'`, `INTERVAL '1 day'`).
        if first.kind == TokenKind::Word
            && hi == lo + 2
            && matches!(self.kind(lo + 1), Some(TokenKind::String(_)))
        {
            return Operand::Value { literal: true, leading_wildcard: false };
        }
        if first.kind == TokenKind::Word
            && hi == lo + 1
            && VALUE_WORDS.iter().any(|v| first.is_word(self.text, v))
        {
            return Operand::Value { literal: true, leading_wildcard: false };
        }
        // A column alone.
        if let Some((column, next)) = self.column_at(lo, hi)
            && next == hi
        {
            out.columns.push(column.clone());
            return Operand::Column { column, wrapper: None, cast: None };
        }
        // A function's call: `name(args)` (its name maybe qualified).
        let mut name_end = lo;
        while name_end < hi && self.tokens[name_end].is_name() {
            name_end += 1;
            if self.kind(name_end) == Some(TokenKind::Dot) {
                name_end += 1;
            } else {
                break;
            }
        }
        if name_end > lo
            && self.kind(name_end) == Some(TokenKind::OpenParen)
            && let Some(close) = self.closing(name_end, hi)
            && close == hi - 1
        {
            {
                let function = self.text[self.range(lo, name_end)].to_owned();
                let args = self.args(name_end + 1, close);
                let mut column: Option<(ColumnRef, Option<String>)> = None;
                let mut columns = 0;
                for &(a, b) in &args {
                    if let Operand::Column { column: c, cast, .. } = self.operand(a, b, out) {
                        columns += 1;
                        column.get_or_insert((c, cast));
                    }
                }
                let upper = function.to_ascii_uppercase();
                if upper == "ANY" || upper == "ALL" || upper == "SOME" || upper == "ARRAY" {
                    return if columns == 0 {
                        Operand::Value { literal: true, leading_wildcard: false }
                    } else {
                        Operand::Other
                    };
                }
                return match column {
                    Some((column, cast)) if columns == 1 => Operand::Column {
                        column,
                        wrapper: Some(Wrapper {
                            function,
                            range: self.range(lo, hi),
                            single: args.len() == 1,
                        }),
                        cast,
                    },
                    Some(_) => Operand::Other,
                    // A function of values (`now()`, `coalesce(1, 2)`) is a value.
                    None => Operand::Value { literal: false, leading_wildcard: false },
                };
            }
        }
        // A list of values (`IN (1, 2)`), an array (`ARRAY[…]`).
        if first.kind == TokenKind::OpenParen || first.is_word(self.text, "ARRAY") {
            let before = out.columns.len();
            self.mention(lo, hi, out);
            let subplan = (lo..hi).any(|i| self.word(i, "SubPlan") || self.word(i, "SELECT"));
            if out.columns.len() == before && !subplan {
                return Operand::Value { literal: true, leading_wildcard: false };
            }
            return Operand::Other;
        }
        self.mention(lo, hi, out);
        Operand::Other
    }

    /// A call's arguments (`lo..hi` inside its parentheses), split at its top-level commas.
    fn args(&self, lo: usize, hi: usize) -> Vec<(usize, usize)> {
        if lo >= hi {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut start = lo;
        for i in lo..hi {
            match self.tokens[i].kind {
                TokenKind::OpenParen => depth += 1,
                TokenKind::CloseParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => {
                    out.push((start, i));
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push((start, hi));
        out
    }
}

/// Words of SQL that are never a column in a condition.
fn is_keyword(word: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "AND", "OR", "NOT", "IS", "IN", "LIKE", "ILIKE", "BETWEEN", "ANY", "ALL", "SOME", "CASE",
        "WHEN", "THEN", "ELSE", "END", "AS", "CAST", "EXISTS", "SELECT", "FROM", "WHERE", "ARRAY",
        "DISTINCT", "SubPlan", "InitPlan", "ESCAPE", "COLLATE", "GLOB", "INTERVAL",
    ];
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
}

#[cfg(test)]
#[path = "../../tests/unit/profile/cond.rs"]
mod tests;
