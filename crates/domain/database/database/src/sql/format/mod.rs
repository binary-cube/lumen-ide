//! SQL laid out again ("Reformat Code"): pure and deterministic, on the lexer's tokens by
//! the text's [`Flavor`].
//!
//! Only the whitespace between tokens changes, and the case of the keywords as chosen;
//! every other token — names, strings, numbers, parameters, comments, PostgreSQL's
//! dollar-quoted bodies — comes out byte for byte, in its order, so what a statement means
//! never changes. That is checked, not hoped: each statement laid out is read again and
//! must give the same tokens (the same statements, read with or without separator lines),
//! else it's left as written. Laying out what was laid out changes nothing (idempotent).
//!
//! The layout: each clause on a line of its own (`SELECT`, `FROM`, each join, `WHERE`,
//! `GROUP BY`…), its list on the clause's line when it fits in the width, else one item a
//! line, indented (or always, [`ListStyle::OnePerLine`]); commas after the items or before
//! them ([`CommaStyle`]); a condition broken before its `AND` / `OR`; a join's `ON` on its
//! line or under it; subqueries, CTE bodies and `CASE` indented; `CREATE TABLE`'s columns
//! one a line; a trigger's body one statement a line; other statements on their line,
//! broken after their commas when too long. A comment stays where it was: after its token
//! on the same line, or on a line of its own. Blank lines between statements are kept (one
//! of several). An unfinished statement (a string left open, a parenthesis not closed) is
//! left as written.

mod doc;
mod layout;
mod words;

use std::ops::Range;

use super::split::{SplitOptions, StatementSpan, split, statement_at};
use super::{Flavor, Token, TokenKind, lex};

/// How the keywords are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum KeywordCase {
    #[default]
    Upper,
    Lower,
    /// As written.
    Preserve,
}

/// Where a list's commas go when it's broken one item a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CommaStyle {
    /// `a,` / `b` — after each item.
    #[default]
    Trailing,
    /// `a` / `, b` — at the start of the next.
    Leading,
}

/// When a list (columns, values, assignments) is broken one item a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ListStyle {
    /// Only when it doesn't fit in the width.
    #[default]
    Fit,
    /// Whenever it has more than one item.
    OnePerLine,
}

/// How SQL is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatOptions {
    pub keyword_case: KeywordCase,
    /// Spaces a step of indentation is (at least one).
    pub indent: usize,
    /// The width lines are kept within, where they can be (columns).
    pub width: usize,
    pub commas: CommaStyle,
    pub lists: ListStyle,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Upper,
            indent: 4,
            width: 120,
            commas: CommaStyle::Trailing,
            lists: ListStyle::Fit,
        }
    }
}

/// A text laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    /// The statements left as written (unfinished, or not safely laid out).
    pub left: usize,
}

/// A part of a text laid out: what replaces `range`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeFormatted {
    pub range: Range<usize>,
    pub text: String,
    pub left: usize,
}

/// Separator lines (`GO`, `/`) kept on their lines whatever the console's setting: read
/// with them, a statement never moves onto or off such a line.
const SPLIT: SplitOptions = SplitOptions { line_separators: true };

/// `text` laid out by `flavor`'s rules.
pub fn format(text: &str, flavor: Flavor, options: &FormatOptions) -> Formatted {
    let tokens = lex(text, flavor);
    let spans = split(text, &tokens, flavor, &SPLIT);
    if tokens.iter().all(|t| t.kind == TokenKind::Whitespace) {
        return Formatted { text: text.to_owned(), left: 0 };
    }
    let mut out = Output::new(text, &tokens, flavor, options, "", 0);
    out.units(&spans, 0..tokens.len());
    let Output { text: mut formatted, left, .. } = out;
    // The last line's break kept (or not), as it was.
    if text.ends_with(['\n', '\r']) {
        formatted.push_str(line_end(text));
    }
    if !same_meaning(text, &formatted, flavor) {
        return Formatted { text: text.to_owned(), left: spans.len() };
    }
    Formatted { text: formatted, left }
}

/// The statements `range` touches laid out (a selection), or the one at the cursor when
/// it's empty (as a console picks the statement to run): what replaces them. `None`: no
/// statement there.
pub fn format_range(
    text: &str,
    range: Range<usize>,
    flavor: Flavor,
    options: &FormatOptions,
) -> Option<RangeFormatted> {
    let tokens = lex(text, flavor);
    let spans = split(text, &tokens, flavor, &SPLIT);
    let chosen: Vec<&StatementSpan> = if range.is_empty() {
        statement_at(text, &spans, range.start).into_iter().collect()
    } else {
        spans.iter().filter(|s| s.range.start < range.end && s.end() > range.start).collect()
    };
    let (first, last) = (chosen.first()?, chosen.last()?);
    let replaced = first.range.start..last.end();
    let token_range = first.tokens.start..token_end(&tokens, last);
    // The lines after the first start as the first's does.
    let line_start = text[..replaced.start].rfind(['\n', '\r']).map_or(0, |p| p + 1);
    let before = &text[line_start..replaced.start];
    let margin = if before.chars().all(|c| c == ' ' || c == '\t') { before } else { "" };
    let mut out = Output::new(text, &tokens, flavor, options, margin, doc::width(before));
    out.units(&spans, token_range);
    let Output { text: formatted, left, .. } = out;
    let original = &text[replaced.clone()];
    if !same_meaning(original, &formatted, flavor) {
        return None;
    }
    Some(RangeFormatted { range: replaced, text: formatted, left })
}

/// The statement at `offset` laid out (see [`format_range`]).
pub fn format_at(
    text: &str,
    offset: usize,
    flavor: Flavor,
    options: &FormatOptions,
) -> Option<RangeFormatted> {
    format_range(text, offset..offset, flavor, options)
}

/// Whether `a` and `b` say the same: the same tokens apart from whitespace (a word's case
/// aside), and the same statements, read with separator lines and without.
pub fn same_meaning(a: &str, b: &str, flavor: Flavor) -> bool {
    let (ta, tb) = (lex(a, flavor), lex(b, flavor));
    if !same_tokens(a, &ta, b, &tb) {
        return false;
    }
    [SPLIT, SplitOptions::default()].iter().all(|options| {
        let sa = split(a, &ta, flavor, options);
        let sb = split(b, &tb, flavor, options);
        let said = |tokens: &[Token], span: &StatementSpan| {
            tokens[span.tokens.clone()].iter().filter(|t| t.kind != TokenKind::Whitespace).count()
        };
        sa.len() == sb.len()
            && sa.iter().zip(&sb).all(|(x, y)| {
                said(&ta, x) == said(&tb, y)
                    && x.incomplete == y.incomplete
                    && x.terminator.is_some() == y.terminator.is_some()
            })
    })
}

/// The same tokens (whitespace aside), in order: the same kind and text (a word's case
/// aside).
fn same_tokens(a: &str, ta: &[Token], b: &str, tb: &[Token]) -> bool {
    let mut xs = ta.iter().filter(|t| t.kind != TokenKind::Whitespace);
    let mut ys = tb.iter().filter(|t| t.kind != TokenKind::Whitespace);
    loop {
        match (xs.next(), ys.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                let (tx, ty) = (x.text(a), y.text(b));
                let same_text =
                    tx == ty || (x.kind == TokenKind::Word && tx.eq_ignore_ascii_case(ty));
                if x.kind != y.kind || x.unterminated != y.unterminated || !same_text {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// How the text's lines end: `\r\n` if it writes them so, else `\n`.
fn line_end(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Where a statement's tokens end, its separator included.
fn token_end(tokens: &[Token], span: &StatementSpan) -> usize {
    match &span.terminator {
        Some(t) => tokens.partition_point(|tok| tok.start < t.start) + 1,
        None => span.tokens.end,
    }
}

/// A text being laid out, statement by statement.
struct Output<'t> {
    source: &'t str,
    tokens: &'t [Token],
    flavor: Flavor,
    options: &'t FormatOptions,
    margin: &'t str,
    /// The column the first line starts at.
    column: usize,
    /// How lines end.
    newline: &'static str,
    text: String,
    left: usize,
}

impl<'t> Output<'t> {
    fn new(
        source: &'t str,
        tokens: &'t [Token],
        flavor: Flavor,
        options: &'t FormatOptions,
        margin: &'t str,
        column: usize,
    ) -> Self {
        let newline = line_end(source);
        Self {
            source,
            tokens,
            flavor,
            options,
            margin,
            column,
            newline,
            text: String::new(),
            left: 0,
        }
    }

    /// The statements and what's between them (comments, stray `;`, separator lines) in
    /// `range` (token indices), each on its line, the blank lines between kept (one).
    fn units(&mut self, spans: &[StatementSpan], range: Range<usize>) {
        let mut spans = spans.iter().filter(|s| s.tokens.start >= range.start).peekable();
        let mut i = range.start;
        // Where the last unit written ended (token index past it).
        let mut previous: Option<usize> = None;
        while i < range.end {
            let token = self.tokens[i];
            if token.kind == TokenKind::Whitespace {
                i += 1;
                continue;
            }
            let span = spans.next_if(|s| s.tokens.start == i);
            let end = span.map_or(i + 1, |s| token_end(self.tokens, s)).min(range.end);
            let breaks = previous.map_or(0, |p| self.breaks(p..i));
            let loose = span.is_none();
            if previous.is_some() {
                if breaks == 0 && loose {
                    // A comment after a `;` on its line, or a `;` after a `;`: kept there.
                    if token.is_comment() {
                        self.text.push(' ');
                    }
                } else {
                    self.newline();
                    if breaks > 1 {
                        self.newline();
                    }
                }
            }
            match span {
                Some(span) => self.statement(span, i..end),
                None => self.text.push_str(token.text(self.source)),
            }
            previous = Some(end);
            i = end;
        }
    }

    fn newline(&mut self) {
        self.text.push_str(self.newline);
        self.text.push_str(self.margin);
    }

    /// The line breaks in the whitespace of tokens `range`.
    fn breaks(&self, range: Range<usize>) -> usize {
        self.tokens[range]
            .iter()
            .filter(|t| t.kind == TokenKind::Whitespace)
            .map(|t| {
                let raw = t.text(self.source);
                raw.matches('\n').count().max(raw.matches('\r').count())
            })
            .sum()
    }

    /// A statement of tokens `range`, laid out (or as written).
    fn statement(&mut self, span: &StatementSpan, range: Range<usize>) {
        let tokens = &self.tokens[range.clone()];
        let start = tokens.first().map_or(0, |t| t.start);
        let end = tokens.last().map_or(start, |t| t.end);
        let original = &self.source[start..end];
        let separator_line = span.terminator.as_ref().is_some_and(|t| {
            self.tokens[range.end - 1].start == t.start
                && self.tokens[range.end - 1].kind != TokenKind::Semicolon
        });
        let column = if self.text.is_empty() { self.column } else { doc::width(self.margin) };
        let laid = (!span.incomplete)
            .then(|| layout::Layout::new(self.source, tokens, self.flavor, self.options))
            .flatten()
            .map(|mut layout| {
                let doc = layout.statement(separator_line);
                doc::print(
                    &doc,
                    doc::Page {
                        width: self.options.width.max(20),
                        indent: self.options.indent.max(1),
                        margin: self.margin,
                        column,
                        newline: self.newline,
                    },
                )
            })
            .filter(|laid| same_meaning(original, laid, self.flavor));
        match laid {
            Some(laid) => self.text.push_str(&laid),
            None => {
                self.left += 1;
                self.text.push_str(original);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/sql/format.rs"]
mod tests;
