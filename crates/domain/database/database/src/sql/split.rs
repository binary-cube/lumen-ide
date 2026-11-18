//! A text's statements: where each starts and ends, found from its tokens.
//!
//! A `;` ends a statement only at the top level: not in a string, a comment, a quoted name or
//! a dollar-quoted body (those are single tokens), not between parentheses, and not in a
//! body that holds statements of its own — a trigger's `BEGIN … END` (SQLite, and MySQL's
//! compound statements) or a `BEGIN ATOMIC … END` (PostgreSQL 14+). Statements holding
//! nothing but comments are skipped.

use std::ops::Range;

use super::Flavor;
use super::lex::{Token, TokenKind};

/// How statements are told apart, beyond `;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SplitOptions {
    /// A line holding only `GO` or `/` ends a statement too (scripts copied from SQL
    /// Server's or Oracle's tools).
    pub line_separators: bool,
}

/// Where a statement is in its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpan {
    /// From its first token to its last (no whitespace, no `;`, no comment around it).
    pub range: Range<usize>,
    /// With the comments written on the lines right before it (what "run the statement"
    /// shows and runs along).
    pub with_comments: Range<usize>,
    /// The separator ending it (`;`, or a `GO` / `/` line), if any.
    pub terminator: Option<Range<usize>>,
    /// Its tokens (indices in the text's tokens): from its first to its last.
    pub tokens: Range<usize>,
    /// Its first word (`SELECT`, `WITH`…), if it starts with one (maybe after `(`).
    pub first_word: Option<Range<usize>>,
    /// It ends inside a string, a comment, a body, a parenthesis or a block.
    pub incomplete: bool,
}

impl StatementSpan {
    /// Where it ends, its separator included.
    pub fn end(&self) -> usize {
        self.terminator.as_ref().map_or(self.range.end, |t| t.end)
    }

    /// Its text in `text`.
    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.range.clone()]
    }

    /// Its first word in `text`, as written.
    pub fn first_word<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.first_word.clone().map(|r| &text[r])
    }
}

/// An open body a `;` doesn't end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// `BEGIN … END`.
    Begin,
    /// `CASE … END`.
    Case,
}

/// The statements of `text` (whose tokens are `tokens`), in order.
pub fn split(
    text: &str,
    tokens: &[Token],
    flavor: Flavor,
    options: &SplitOptions,
) -> Vec<StatementSpan> {
    let mut out = Vec::new();
    let mut current = Current::default();
    // Whether a line break came since the last statement's end (a comment on the same line
    // as a `;` belongs to the statement before).
    let mut new_line = true;
    let mut comment_start: Option<usize> = None;

    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        match token.kind {
            TokenKind::Whitespace => {
                if token.text(text).contains(['\n', '\r']) {
                    new_line = true;
                }
            }
            TokenKind::LineComment | TokenKind::BlockComment => {
                if current.first.is_none() && new_line && comment_start.is_none() {
                    comment_start = Some(token.start);
                }
            }
            TokenKind::Semicolon if current.at_top() => {
                current.finish(&mut out, Some(token.range()), comment_start.take());
                current = Current::default();
                new_line = false;
            }
            _ if options.line_separators
                && current.at_top()
                && is_separator_line(text, tokens, i) =>
            {
                current.finish(&mut out, Some(token.range()), comment_start.take());
                current = Current::default();
                new_line = false;
            }
            _ => {
                current.take(text, tokens, i, flavor);
            }
        }
        i += 1;
    }
    current.finish(&mut out, None, comment_start);
    out
}

/// A statement being read.
#[derive(Debug, Default)]
struct Current {
    /// Its first and last significant tokens (index, token).
    first: Option<(usize, Token)>,
    last: Option<(usize, Token)>,
    first_word: Option<Range<usize>>,
    depth: usize,
    blocks: Vec<Block>,
    /// Its words at the top level so far (only the first few matter).
    words: usize,
    starts_with_create: bool,
    trigger: bool,
    unterminated: bool,
}

impl Current {
    /// Whether a `;` here ends it.
    fn at_top(&self) -> bool {
        self.depth == 0 && self.blocks.is_empty()
    }

    fn take(&mut self, text: &str, tokens: &[Token], i: usize, flavor: Flavor) {
        let token = tokens[i];
        if self.first.is_none() {
            self.first = Some((i, token));
        }
        self.last = Some((i, token));
        self.unterminated |= token.unterminated;
        match token.kind {
            TokenKind::OpenParen => self.depth += 1,
            TokenKind::CloseParen => self.depth = self.depth.saturating_sub(1),
            TokenKind::Word => self.word(text, tokens, i, flavor),
            _ => {}
        }
    }

    fn word(&mut self, text: &str, tokens: &[Token], i: usize, flavor: Flavor) {
        let token = tokens[i];
        let word = token.text(text);
        let is = |k: &str| word.eq_ignore_ascii_case(k);
        if self.first_word.is_none() {
            self.first_word = Some(token.range());
            self.starts_with_create = is("CREATE");
        }
        if self.depth == 0 && self.blocks.is_empty() {
            self.words += 1;
            // `CREATE [OR REPLACE] [TEMP | TEMPORARY] TRIGGER`.
            if self.starts_with_create && self.words <= 5 && is("TRIGGER") {
                self.trigger = true;
            }
        }
        if self.blocks.is_empty() {
            if self.starts_with_create && is("BEGIN") {
                let atomic =
                    next_word(text, tokens, i).is_some_and(|w| w.eq_ignore_ascii_case("ATOMIC"));
                // PostgreSQL's triggers run a function: they have no body.
                let trigger_body = self.trigger && !flavor.is_postgres();
                if (atomic && flavor.begin_atomic()) || trigger_body {
                    self.blocks.push(Block::Begin);
                }
            }
            return;
        }
        if is("CASE") {
            // `END CASE` (MySQL) closes a case, it doesn't open one.
            let after_end =
                prev_word(text, tokens, i).is_some_and(|w| w.eq_ignore_ascii_case("END"));
            if !after_end {
                self.blocks.push(Block::Case);
            }
        } else if is("BEGIN") {
            self.blocks.push(Block::Begin);
        } else if is("END") {
            // MySQL's `END IF`, `END LOOP`, `END WHILE`, `END REPEAT` close what was never
            // counted; `END CASE` closes a case.
            let next = next_word(text, tokens, i);
            let ends_other = next.is_some_and(|w| {
                ["IF", "LOOP", "WHILE", "REPEAT"].iter().any(|k| w.eq_ignore_ascii_case(k))
            });
            if !(ends_other && flavor == Flavor::MySql) {
                self.blocks.pop();
            }
        }
    }

    fn finish(
        &self,
        out: &mut Vec<StatementSpan>,
        terminator: Option<Range<usize>>,
        comment_start: Option<usize>,
    ) {
        let (Some((first_i, first)), Some((last_i, last))) = (self.first, self.last) else {
            return;
        };
        let range = first.start..last.end;
        out.push(StatementSpan {
            with_comments: comment_start.unwrap_or(range.start)..range.end,
            range,
            terminator,
            tokens: first_i..last_i + 1,
            first_word: self.first_word.clone().filter(|w| {
                // Only when it's the first thing said (maybe after parentheses).
                first.kind == TokenKind::OpenParen || w.start == first.start
            }),
            incomplete: self.unterminated || !self.at_top(),
        });
    }
}

fn next_word<'a>(text: &'a str, tokens: &[Token], i: usize) -> Option<&'a str> {
    let t = tokens[i + 1..].iter().find(|t| !t.is_trivia())?;
    (t.kind == TokenKind::Word).then(|| t.text(text))
}

fn prev_word<'a>(text: &'a str, tokens: &[Token], i: usize) -> Option<&'a str> {
    let t = tokens[..i].iter().rev().find(|t| !t.is_trivia())?;
    (t.kind == TokenKind::Word).then(|| t.text(text))
}

/// Whether `tokens[i]` is a `GO` or a `/` alone on its line.
fn is_separator_line(text: &str, tokens: &[Token], i: usize) -> bool {
    let token = tokens[i];
    let separator =
        token.is_word(text, "GO") || (token.kind == TokenKind::Operator && token.text(text) == "/");
    if !separator {
        return false;
    }
    let line_start = text[..token.start].rfind(['\n', '\r']).map_or(0, |p| p + 1);
    let line_end = text[token.end..].find(['\n', '\r']).map_or(text.len(), |p| token.end + p);
    text[line_start..token.start].trim().is_empty() && text[token.end..line_end].trim().is_empty()
}

/// The statement the cursor at `offset` is in, as a console picks it: the one
/// holding it (its comments and its `;` included; at the very end of one and the start of
/// the next, the next); else, on the rest of a statement's line or on the line right after
/// it with nothing typed between, that one; else none.
pub fn statement_at<'a>(
    text: &str,
    statements: &'a [StatementSpan],
    offset: usize,
) -> Option<&'a StatementSpan> {
    // The first statement ending at or after the cursor.
    let i = statements.partition_point(|s| s.end() < offset);
    if let Some(s) = statements.get(i)
        && s.with_comments.start <= offset
    {
        // At a statement's end and the next's start, the next.
        let next =
            statements.get(i + 1).filter(|n| s.end() == offset && n.with_comments.start == offset);
        return Some(next.unwrap_or(s));
    }
    let before = statements.get(i.checked_sub(1)?)?;
    let gap = text.get(before.end()..offset)?;
    let breaks = gap.matches('\n').count().max(gap.matches('\r').count());
    (gap.trim().is_empty() && breaks <= 1).then_some(before)
}

#[cfg(test)]
#[path = "../../tests/unit/sql/split.rs"]
mod tests;
