//! A statement's parameters (`?`, `?1`, `$1`, `:name`, `@name`, `$name`) and their values
//! written into its text as literals, by the dialect's own quoting: the same on every engine
//! and through the drivers' protocol, and what runs is exactly what's shown and kept.
//!
//! Parameters are tokens: one in a string, a comment, a quoted name or a dollar-quoted body
//! is just text. A [`BindMap`] takes a position in the text sent (an engine's error) back to
//! the document.

use std::collections::HashMap;
use std::ops::Range;

use super::Flavor;
use super::lex::{ParamKind, Token, TokenKind};
use crate::dialect::Dialect;
use crate::{DbError, Value};

/// Which ways of writing a parameter count as one (the rest are left as written).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamStyles {
    /// `?`.
    pub positional: bool,
    /// `?1`, `$1`.
    pub numbered: bool,
    /// `:name`.
    pub colon: bool,
    /// `@name`.
    pub at: bool,
    /// `$name`.
    pub dollar: bool,
}

impl ParamStyles {
    /// What `flavor`'s engine (or its usual clients) reads as parameters. PostgreSQL leaves
    /// `?` alone (`jsonb ? 'key'` is an operator); MySQL's `@name` is a user variable.
    pub fn of(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Postgres { .. } => {
                Self { positional: false, numbered: true, colon: true, at: false, dollar: false }
            }
            Flavor::Sqlite => {
                Self { positional: true, numbered: true, colon: true, at: true, dollar: true }
            }
            Flavor::MySql => {
                Self { positional: true, numbered: false, colon: true, at: false, dollar: false }
            }
            Flavor::Generic => {
                Self { positional: true, numbered: true, colon: true, at: false, dollar: false }
            }
        }
    }

    pub fn all() -> Self {
        Self { positional: true, numbered: true, colon: true, at: true, dollar: true }
    }
}

/// What a parameter is called.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParamName {
    /// The `n`-th `?` (from 1).
    Positional(usize),
    /// `?n`, `$n`.
    Numbered(usize),
    /// `:name`, `@name`, `$name`: the name, without its sign (`:id` and `@id` are one).
    Named(String),
}

/// A parameter and where it's written (byte ranges in the text), in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: ParamName,
    pub places: Vec<Range<usize>>,
}

/// The parameters among `tokens` (read from `text`), each once, in the order they first
/// appear; `?` are numbered in their order among `tokens` (give them one statement's).
pub fn params(text: &str, tokens: &[Token], styles: &ParamStyles) -> Vec<Parameter> {
    let mut out: Vec<Parameter> = Vec::new();
    let mut positional = 0;
    for token in tokens {
        let TokenKind::Parameter(kind) = token.kind else { continue };
        let raw = token.text(text);
        let name = match kind {
            ParamKind::Positional if styles.positional => {
                positional += 1;
                ParamName::Positional(positional)
            }
            ParamKind::Numbered if styles.numbered => match raw[1..].parse() {
                Ok(n) => ParamName::Numbered(n),
                Err(_) => continue,
            },
            ParamKind::Named => {
                let allowed = match raw.as_bytes()[0] {
                    b':' => styles.colon,
                    b'@' => styles.at,
                    b'$' => styles.dollar,
                    _ => false,
                };
                if !allowed {
                    continue;
                }
                ParamName::Named(raw[1..].to_owned())
            }
            _ => continue,
        };
        match out.iter_mut().find(|p| p.name == name) {
            Some(p) => p.places.push(token.range()),
            None => out.push(Parameter { name, places: vec![token.range()] }),
        }
    }
    out
}

/// A parameter's value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// Written as the dialect's literal (quoted safely).
    Literal(Value),
    /// Written as typed: an expression (`now()`, `ARRAY[1, 2]`), the user's choice.
    Raw(String),
}

/// Why a statement's values couldn't be written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// A parameter has no value (never a silent `NULL`).
    Missing(ParamName),
    /// The dialect can't write its value.
    Refused(ParamName, DbError),
}

/// A statement with its values written in, and how its positions map back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub text: String,
    pub map: BindMap,
}

/// Where the text sent came from: its offsets taken back to the document's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BindMap {
    /// Where the statement starts in the document.
    pub start: usize,
    /// What was replaced, in order: its range in the text sent and in the statement as
    /// written (both from the statement's start).
    pub replaced: Vec<(Range<usize>, Range<usize>)>,
}

impl BindMap {
    /// A statement sent as written, starting at `start` in the document.
    pub fn plain(start: usize) -> Self {
        Self { start, replaced: Vec::new() }
    }

    /// The document's offset for `offset` in the text sent: inside a value, where its
    /// parameter starts.
    pub fn to_document(&self, offset: usize) -> usize {
        let mut shift: isize = 0;
        for (sent, original) in &self.replaced {
            if offset < sent.start {
                break;
            }
            if offset < sent.end {
                return self.start + original.start;
            }
            shift = original.end as isize - sent.end as isize;
        }
        self.start + (offset as isize + shift).max(0) as usize
    }
}

/// The text of `range` in `document` with `parameters` (found in `document`; those outside
/// `range` ignored) replaced by their `values`, written by `dialect`.
pub fn bind(
    document: &str,
    range: Range<usize>,
    parameters: &[Parameter],
    values: &HashMap<ParamName, ParamValue>,
    dialect: &dyn Dialect,
) -> Result<Bound, BindError> {
    let mut places: Vec<(Range<usize>, &ParamName)> = parameters
        .iter()
        .flat_map(|p| p.places.iter().map(move |r| (r.clone(), &p.name)))
        .filter(|(r, _)| range.start <= r.start && r.end <= range.end)
        .collect();
    places.sort_by_key(|(r, _)| r.start);

    let mut text = String::with_capacity(range.len() + places.len() * 8);
    let mut map = BindMap::plain(range.start);
    let mut at = range.start;
    for (place, name) in places {
        text.push_str(&document[at..place.start]);
        let literal = match values.get(name) {
            None => return Err(BindError::Missing(name.clone())),
            Some(ParamValue::Raw(raw)) => raw.clone(),
            Some(ParamValue::Literal(value)) => {
                dialect.literal(value).map_err(|e| BindError::Refused(name.clone(), e))?
            }
        };
        let sent_start = text.len();
        text.push_str(&literal);
        map.replaced
            .push((sent_start..text.len(), place.start - range.start..place.end - range.start));
        at = place.end;
    }
    text.push_str(&document[at..range.end]);
    Ok(Bound { text, map })
}

#[cfg(test)]
#[path = "../../tests/unit/sql/params.rs"]
mod tests;
