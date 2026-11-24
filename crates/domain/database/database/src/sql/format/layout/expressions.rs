//! Expressions: conditions broken before `AND` / `OR`, parentheses, `CASE` blocks, and
//! what goes between two tokens on a line.

use std::borrow::Cow;
use std::ops::Range;

use crate::sql::TokenKind;

use super::{Doc, KeywordCase, Layout, NONE, Sep, cased, words};

impl<'a, 'o> Layout<'a, 'o> {
    /// A condition, broken before its `AND` / `OR` (not a `BETWEEN`'s `AND`).
    pub(super) fn cond(&mut self, r: Range<usize>) -> Doc<'a> {
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
    pub(super) fn expr(&mut self, r: Range<usize>) -> Doc<'a> {
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

    pub(super) fn paren(&mut self, open: usize, close: usize) -> Doc<'a> {
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
    pub(super) fn case(&mut self, case: usize, end: usize) -> Doc<'a> {
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
    pub(super) fn before_paren(&self, a: usize) -> Option<Doc<'a>> {
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
    pub(super) fn tok(&self, i: usize) -> Doc<'a> {
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
