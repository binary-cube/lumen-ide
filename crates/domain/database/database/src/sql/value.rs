//! Whose value is written at a place of a statement, for completion to offer what that
//! column allows (an enum's literals): the column compared (`status = |`, `status <> |`,
//! `status IN ('a', |`), set (`SET status = |`) or inserted (`INSERT INTO t (a, status)
//! VALUES (1, |`, or by its place among the table's columns without a list), and the
//! string literal the cursor is in, if any (`status = 'ac|`).
//!
//! Pure, on the statement's tokens ([`super::lex`]); tolerant of what's half typed.

use std::ops::Range;

use super::identifier;
use super::lex::{Token, TokenKind};

/// The column whose value goes at a place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueOf {
    /// A column by name, with its qualifier as written (`o.status`: `["o"]`).
    Column { qualifier: Vec<String>, name: String },
    /// The `position`th value (from 0) of an `INSERT` into the table `table` names (its
    /// qualifier as written) without a column list: its column in the table's order.
    Position { table: Vec<String>, position: usize },
}

/// The comparisons a value follows.
const COMPARISONS: &[&str] = &["=", "<>", "!=", "=="];

/// The string literal `offset` is in (`'ac|`, `'ac|tive'`), when it's a plain one
/// (starting with `'`): its token.
pub fn open_literal(text: &str, tokens: &[Token], offset: usize) -> Option<Token> {
    tokens
        .iter()
        .find(|t| {
            matches!(t.kind, TokenKind::String(_))
                && t.start < offset
                && (offset < t.end || (offset == t.end && t.unterminated))
        })
        .filter(|t| text[t.start..].starts_with('\''))
        .copied()
}

/// The column whose value is written at `at` of the statement `statement` (its tokens),
/// if one is (see the module's doc). `at` is where the value starts (the word typed or the
/// literal left out).
pub fn value_of(text: &str, statement: &[Token], at: usize) -> Option<ValueOf> {
    let sig: Vec<Token> =
        statement.iter().filter(|t| !t.is_trivia() && t.end <= at).copied().collect();
    let last = sig.last()?;
    let n = sig.len();
    // `status = |`, `SET status = |`.
    if last.kind == TokenKind::Operator && COMPARISONS.contains(&last.text(text)) {
        let (qualifier, name) = column_before(text, &sig[..n - 1])?;
        return Some(ValueOf::Column { qualifier, name });
    }
    // In a list: `IN (…, |`, `VALUES (…, |`.
    let (open, position) = list_open(text, &sig)?;
    let before = &sig[..open];
    let word = |i: usize| before.get(i).filter(|t| t.kind == TokenKind::Word).map(|t| t.text(text));
    let k = before.len();
    if k >= 1 && word(k - 1).is_some_and(|w| w.eq_ignore_ascii_case("IN")) {
        let mut end = k - 1;
        if end >= 1 && word(end - 1).is_some_and(|w| w.eq_ignore_ascii_case("NOT")) {
            end -= 1;
        }
        let (qualifier, name) = column_before(text, &before[..end])?;
        return Some(ValueOf::Column { qualifier, name });
    }
    // `VALUES (…), (…, |`: the rows before it skipped back to `VALUES`.
    let mut v = k;
    loop {
        let w = v.checked_sub(1)?;
        if word(w).is_some_and(|w| w.eq_ignore_ascii_case("VALUES")) {
            break;
        }
        // `(…) ,` before this row.
        if before[w].kind != TokenKind::Comma
            || before.get(w.checked_sub(1)?)?.kind != TokenKind::CloseParen
        {
            return None;
        }
        v = matching_open(text, before, w - 1)?;
    }
    insert_column(text, &before[..v - 1], position)
}

/// The column named right before the end of `sig` (`status`, `o.status`, `"Status"`).
fn column_before(text: &str, sig: &[Token]) -> Option<(Vec<String>, String)> {
    let mut i = sig.len().checked_sub(1)?;
    if !sig[i].is_name() {
        return None;
    }
    let mut parts = vec![identifier(text, &sig[i]).into_owned()];
    while i >= 2 && sig[i - 1].kind == TokenKind::Dot && sig[i - 2].is_name() {
        i -= 2;
        parts.push(identifier(text, &sig[i]).into_owned());
    }
    parts.reverse();
    let name = parts.pop()?;
    Some((parts, name))
}

/// The `(` of the list the end of `sig` is in (its index), and how many of the list's
/// commas come before (the value's place).
fn list_open(text: &str, sig: &[Token]) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut commas = 0;
    for (i, t) in sig.iter().enumerate().rev() {
        match t.kind {
            TokenKind::CloseParen => depth += 1,
            TokenKind::OpenParen if depth == 0 => return Some((i, commas)),
            TokenKind::OpenParen => depth -= 1,
            TokenKind::Comma if depth == 0 => commas += 1,
            TokenKind::Semicolon => return None,
            _ => {}
        }
        let _ = text;
    }
    None
}

/// The `(` matching the `)` at `close` of `sig`.
fn matching_open(text: &str, sig: &[Token], close: usize) -> Option<usize> {
    list_open(text, &sig[..close]).map(|(open, _)| open)
}

/// The column the `position`th value of `INSERT INTO t [(columns)]` goes into (`sig`: the
/// statement up to its `VALUES`).
fn insert_column(text: &str, sig: &[Token], position: usize) -> Option<ValueOf> {
    let into = sig.iter().position(|t| t.is_word(text, "INTO"))?;
    let rest = &sig[into + 1..];
    // The table's name: names and dots.
    let mut table = Vec::new();
    let mut i = 0;
    while let Some(t) = rest.get(i) {
        if t.is_name() && (table.is_empty() || rest[i - 1].kind == TokenKind::Dot) {
            table.push(identifier(text, t).into_owned());
        } else if t.kind != TokenKind::Dot {
            break;
        }
        i += 1;
    }
    if table.is_empty() {
        return None;
    }
    match rest.get(i) {
        Some(t) if t.kind == TokenKind::OpenParen => {
            let names: Vec<String> = rest[i + 1..]
                .iter()
                .take_while(|t| t.kind != TokenKind::CloseParen)
                .filter(|t| t.is_name())
                .map(|t| identifier(text, t).into_owned())
                .collect();
            let name = names.get(position)?.clone();
            Some(ValueOf::Column { qualifier: Vec::new(), name })
        }
        _ => Some(ValueOf::Position { table, position }),
    }
}

/// The values of an enum type as MySQL writes it (`enum('S','M','it''s')`, `set(…)` not
/// included): in their order, unquoted.
pub fn enum_values(type_name: &str) -> Option<Vec<String>> {
    let t = type_name.trim();
    let open = t.find('(')?;
    if !t[..open].trim().eq_ignore_ascii_case("enum") || !t.ends_with(')') {
        return None;
    }
    let inner = &t[open + 1..t.len() - 1];
    let mut values = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
            chars.next();
        }
        match chars.next() {
            None => break,
            Some('\'') => {}
            Some(_) => return None,
        }
        let mut value = String::new();
        loop {
            match chars.next()? {
                '\'' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    value.push('\'');
                }
                '\'' => break,
                '\\' => value.push(chars.next()?),
                c => value.push(c),
            }
        }
        values.push(value);
    }
    (!values.is_empty()).then_some(values)
}

/// `value` as a string literal (`'it''s'`).
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The literal's range to replace: from its quote to the cursor, or to its end once closed.
pub fn literal_range(token: &Token, offset: usize) -> Range<usize> {
    if token.unterminated { token.start..offset } else { token.start..token.end }
}

#[cfg(test)]
#[path = "../../tests/unit/sql/value.rs"]
mod tests;
