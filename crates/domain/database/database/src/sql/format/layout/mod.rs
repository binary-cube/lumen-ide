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

mod expressions;
mod queries;
mod reading;
mod statements;

use reading::read;

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
