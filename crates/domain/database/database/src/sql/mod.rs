//! SQL text, as the console reads it before anything runs: pure rules, no engine, no grammar
//! (the editor's coloring is tree-sitter's; running never depends on it).
//!
//! [`lex`] reads a text as tokens by its [`Flavor`]'s rules (strings, quoted names,
//! comments, PostgreSQL's dollar-quoted bodies, parameters); [`split`] finds its statements
//! ([`StatementSpan`]: `;` only at the top level, a trigger's `BEGIN … END` and a
//! `BEGIN ATOMIC` body kept whole) and [`statement_at`] the one under the cursor;
//! [`classify`] tells what a statement does ([`StatementKind`]: reads, writes, changes the
//! schema…); [`params`] lists its parameters and [`bind`] writes their values in as literals
//! (a [`BindMap`] taking positions back); [`mask_secrets`] hides the passwords it holds (for
//! the log and the history); [`errors::locate`] puts an engine's error position back in the
//! document; [`fingerprint`] is a statement without its values (the same query run again);
//! [`scope`] is what a statement has in scope at the cursor (its tables and aliases, CTEs,
//! derived tables, what fits there), for completion; [`resolve`] every name of a statement
//! with what it is (a table, an alias, a column and its qualifier…), for going to a
//! definition, the hover card and the schema's diagnostics; [`format`] lays a text out again
//! ("Reformat Code": only whitespace and the keywords' case change, checked).
//! The dangers a statement holds are [`crate::danger::statement_dangers`].

pub mod classify;
pub mod editable;
pub mod errors;
pub mod fingerprint;
pub mod format;
pub mod lex;
pub mod params;
pub mod resolve;
pub mod scope;
pub mod secrets;
pub mod split;
pub mod tables;
pub mod value;

pub use classify::{DdlKind, StatementKind, TxControl, WriteKind, classify};
pub use editable::{QueryShape, SelectItem, query_shape};
pub use errors::{EnginePosition, TextPosition, locate};
pub use fingerprint::{Fingerprint, fingerprint};
pub use format::{
    CommaStyle, FormatOptions, Formatted, KeywordCase, ListStyle, RangeFormatted, format,
    format_at, format_range,
};
pub use lex::{ParamKind, StringKind, Token, TokenKind, lex};
pub use params::{
    BindError, BindMap, Bound, ParamName, ParamStyles, ParamValue, Parameter, bind, params,
};
pub use resolve::{LevelScope, Name, NameRole, Resolution, resolve, resolve_at};
pub use scope::{
    Clause, Cte, Expect, KeywordPlace, OutputColumn, QueryScope, ScopeTable, Star, TableSource,
    scope, scope_in,
};
pub use secrets::mask_secrets;
pub use split::{SplitOptions, StatementSpan, split, statement_at};
pub use tables::{created_tables, query_table, rerunnable, written_tables};

use std::borrow::Cow;

use crate::TableRef;

/// Whose SQL a text is: what its lexer accepts (quotes, comments, bodies, parameters) and
/// how its statements end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavor {
    /// The standard's: `"x"` names, `'…'` strings, `--` and `/* */` comments.
    #[default]
    Generic,
    /// SQLite: `[x]` and `` `x` `` names too, `?NNN`, `:x`, `@x` and `$x` parameters,
    /// triggers' `BEGIN … END` bodies.
    Sqlite,
    /// PostgreSQL (of `version`, `server_version_num`; `None`: unknown): `E'…'` strings,
    /// `$tag$ … $tag$` bodies, nested block comments, `::` casts, `$1` parameters,
    /// `BEGIN ATOMIC … END` bodies (14 and later).
    Postgres { version: Option<u32> },
    /// MySQL / MariaDB (for the drivers to come): `` `x` `` names, `"…"` strings,
    /// backslash escapes, `#` comments.
    MySql,
}

impl Flavor {
    pub fn is_postgres(self) -> bool {
        matches!(self, Flavor::Postgres { .. })
    }

    /// Whether `BEGIN ATOMIC … END` bodies exist (PostgreSQL 14+, the standard's; an
    /// unknown version counts as having them).
    pub fn begin_atomic(self) -> bool {
        match self {
            Flavor::Postgres { version } => version.is_none_or(|v| v >= 140_000),
            Flavor::Generic => true,
            Flavor::Sqlite | Flavor::MySql => false,
        }
    }
}

/// A text's tokens and statements, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub tokens: Vec<Token>,
    pub statements: Vec<StatementSpan>,
}

impl Parsed {
    /// The tokens of `statement` (one of [`Parsed::statements`]).
    pub fn tokens_of(&self, statement: &StatementSpan) -> &[Token] {
        &self.tokens[statement.tokens.clone()]
    }
}

/// `text`'s tokens and statements, by `flavor`'s rules.
pub fn parse(text: &str, flavor: Flavor, options: &SplitOptions) -> Parsed {
    let tokens = lex(text, flavor);
    let statements = split(text, &tokens, flavor, options);
    Parsed { tokens, statements }
}

/// The tokens that say something (no whitespace, no comment), with their depth in
/// parentheses (`0`: the statement's own level).
pub(crate) fn significant(tokens: &[Token]) -> Vec<(Token, usize)> {
    let mut depth = 0usize;
    let mut out = Vec::with_capacity(tokens.len() / 2 + 1);
    for &token in tokens.iter().filter(|t| !t.is_trivia()) {
        match token.kind {
            TokenKind::OpenParen => {
                out.push((token, depth));
                depth += 1;
            }
            TokenKind::CloseParen => {
                depth = depth.saturating_sub(1);
                out.push((token, depth));
            }
            _ => out.push((token, depth)),
        }
    }
    out
}

/// A name as it names its object: a quoted one without its quotes (doubled quotes made
/// single), a word as written.
pub fn identifier<'a>(text: &'a str, token: &Token) -> Cow<'a, str> {
    let raw = token.text(text);
    if token.kind != TokenKind::QuotedIdent {
        return Cow::Borrowed(raw);
    }
    let raw = raw.strip_prefix("U&").or_else(|| raw.strip_prefix("u&")).unwrap_or(raw);
    let (open, close) = match raw.as_bytes().first() {
        Some(b'[') => ('[', ']'),
        Some(b'`') => ('`', '`'),
        _ => ('"', '"'),
    };
    let inner = raw.strip_prefix(open).unwrap_or(raw);
    let inner = if token.unterminated { inner } else { inner.strip_suffix(close).unwrap_or(inner) };
    if open == '[' {
        return Cow::Borrowed(inner);
    }
    let doubled: String = [close, close].iter().collect();
    if inner.contains(&doubled) {
        Cow::Owned(inner.replace(&doubled, &close.to_string()))
    } else {
        Cow::Borrowed(inner)
    }
}

/// The qualified name starting at `sig[at]` (`name`, `schema.name`, `db.schema.name`) and
/// the index after it; `None` when no name is there.
pub(crate) fn qualified_name(
    text: &str,
    sig: &[(Token, usize)],
    at: usize,
) -> Option<(TableRef, usize)> {
    let (first, _) = sig.get(at)?;
    if !first.is_name() {
        return None;
    }
    let mut parts = vec![identifier(text, first).into_owned()];
    let mut i = at + 1;
    while let (Some((dot, _)), Some((next, _))) = (sig.get(i), sig.get(i + 1)) {
        if dot.kind != TokenKind::Dot || !next.is_name() {
            break;
        }
        parts.push(identifier(text, next).into_owned());
        i += 2;
    }
    let name = parts.pop()?;
    let table = match parts.pop() {
        Some(schema) => TableRef::in_schema(schema, name),
        None => TableRef::new(name),
    };
    Some((table, i))
}

#[cfg(test)]
#[path = "../../tests/unit/sql.rs"]
mod tests;
