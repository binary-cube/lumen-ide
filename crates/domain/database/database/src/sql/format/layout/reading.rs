//! The statement read before it's laid out: the tokens that say something with their
//! comments, the pairs matched, the keywords and the types marked, the words around a
//! token, groups skipped and lists split at their commas.

use std::ops::Range;

use crate::sql::{Token, TokenKind};

use super::{Layout, MAX_DEPTH, NONE, Note, Sig, words};

impl<'a, 'o> Layout<'a, 'o> {
    pub(super) fn match_pairs(&mut self) -> Option<()> {
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
    pub(super) fn mark_keywords(&mut self) {
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
    pub(super) fn mark_types(&mut self) {
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

    pub(super) fn kind_at(&self, i: usize) -> Option<TokenKind> {
        self.sig.get(i).map(|s| s.kind)
    }

    /// The word at `i`, uppercase, if it isn't a qualified name's part (`t.order`).
    pub(super) fn name_free(&self, i: usize) -> Option<&str> {
        let s = self.sig.get(i)?;
        let dotted = |j: usize| self.kind_at(j) == Some(TokenKind::Dot);
        if s.kind != TokenKind::Word || dotted(i.wrapping_sub(1)) || dotted(i + 1) {
            return None;
        }
        s.upper.as_deref()
    }

    /// The word right before `i`, uppercase.
    pub(super) fn prev_upper(&self, i: usize) -> Option<&str> {
        self.sig.get(i.checked_sub(1)?)?.upper.as_deref()
    }

    /// Past the group starting at `j` (a parenthesis, a bracket, a `CASE` block) if it
    /// ends before `end`, else past `j`.
    pub(super) fn skip(&self, j: usize, end: usize) -> usize {
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
    pub(super) fn split_commas(&self, r: Range<usize>) -> (Vec<Range<usize>>, Vec<usize>) {
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
}

/// The tokens that say something, each with its comments; `None` when the statement is
/// left as written.
pub(super) fn read<'a>(text: &'a str, tokens: &[Token]) -> Option<Vec<Sig<'a>>> {
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
