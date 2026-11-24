//! A statement by its kind: `EXPLAIN`, `CREATE` (a trigger's body, a table's columns, a
//! view's query), any other on its line broken after its commas.

use std::ops::Range;

use crate::sql::TokenKind;

use super::{CommaStyle, Doc, Layout, NONE, Sep};

impl<'a, 'o> Layout<'a, 'o> {
    pub(super) fn stmt(&mut self, r: Range<usize>) -> Doc<'a> {
        if r.is_empty() {
            return Doc::seq(Vec::new());
        }
        match self.name_free(r.start) {
            Some(
                "SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES" | "MERGE" | "REPLACE",
            ) => self.query(r, Sep::Hard),
            Some("EXPLAIN") => self.explain(r),
            Some("CREATE") => self.create(r),
            _ if self.paren_is_query(r.start) => self.query(r, Sep::Hard),
            _ => self.inline(r),
        }
    }

    /// `EXPLAIN …` on its line, the statement explained under it.
    fn explain(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut j = r.start + 1;
        while j < r.end {
            if matches!(
                self.name_free(j),
                Some("SELECT" | "WITH" | "INSERT" | "UPDATE" | "DELETE" | "VALUES" | "MERGE")
            ) {
                let head = self.expr(r.start..j);
                return Doc::seq(vec![head, Doc::Hard, self.query(j..r.end, Sep::Hard)]);
            }
            j = self.skip(j, r.end);
        }
        self.inline(r)
    }

    fn create(&mut self, r: Range<usize>) -> Doc<'a> {
        if let Some(begin) = self.body_begin(r.clone()) {
            return self.create_with_body(r, begin);
        }
        if let Some(p) = self.table_columns(r.clone()) {
            return self.create_table(r, p);
        }
        if let Some(a) = self.as_query(r.clone()) {
            let head = self.expr(r.start..a + 1);
            return Doc::seq(vec![head, Doc::Hard, self.query(a + 1..r.end, Sep::Hard)]);
        }
        self.inline(r)
    }

    /// A trigger's `BEGIN … END` body (or a `BEGIN ATOMIC` one) holding statements of its
    /// own, ending the statement: where its `BEGIN` is.
    fn body_begin(&self, r: Range<usize>) -> Option<usize> {
        let last = r.end.checked_sub(1)?;
        if self.name_free(last) != Some("END") {
            return None;
        }
        let trigger =
            (r.start..r.end.min(r.start + 6)).any(|j| self.name_free(j) == Some("TRIGGER"));
        let mut begin = None;
        let mut j = r.start + 1;
        while j < last {
            if self.name_free(j) == Some("BEGIN") {
                if begin.is_some() {
                    // Blocks inside (MySQL's): left on their lines as they are.
                    return None;
                }
                let atomic = self.name_free(j + 1) == Some("ATOMIC");
                if (atomic && self.flavor.begin_atomic()) || (trigger && !self.flavor.is_postgres())
                {
                    begin = Some(j);
                } else {
                    return None;
                }
            }
            j = self.skip(j, last);
        }
        begin
    }

    fn create_with_body(&mut self, r: Range<usize>, begin: usize) -> Doc<'a> {
        let end = r.end - 1;
        let atomic = self.name_free(begin + 1) == Some("ATOMIC");
        let mut docs = vec![self.inline(r.start..begin), Doc::Hard, self.tok(begin)];
        if atomic {
            docs.extend([Doc::Space, self.tok(begin + 1)]);
        }
        let mut inner = Vec::new();
        let mut start = begin + 1 + usize::from(atomic);
        let mut j = start;
        while j < end {
            if self.sig[j].kind == TokenKind::Semicolon {
                inner.extend([Doc::Hard, self.stmt(start..j), self.tok(j)]);
                start = j + 1;
            }
            j = self.skip(j, end);
        }
        if start < end {
            inner.extend([Doc::Hard, self.stmt(start..end)]);
        }
        docs.extend([Doc::indent(inner), Doc::Hard, self.tok(end)]);
        Doc::seq(docs)
    }

    /// `CREATE … TABLE [IF NOT EXISTS] name (`: where its columns' `(` is.
    pub(super) fn table_columns(&self, r: Range<usize>) -> Option<usize> {
        if self.name_free(r.start) != Some("CREATE") {
            return None;
        }
        let mut j = r.start + 1;
        while matches!(
            self.name_free(j),
            Some("OR" | "REPLACE" | "TEMP" | "TEMPORARY" | "UNLOGGED" | "GLOBAL" | "LOCAL")
        ) {
            j += 1;
        }
        if self.name_free(j) != Some("TABLE") {
            return None;
        }
        j += 1;
        if self.name_free(j) == Some("IF") {
            j += 3;
        }
        if !self.sig.get(j)?.is_name() {
            return None;
        }
        j += 1;
        while self.kind_at(j) == Some(TokenKind::Dot) && self.sig.get(j + 1)?.is_name() {
            j += 2;
        }
        let close = *self.close.get(j)?;
        (self.kind_at(j) == Some(TokenKind::OpenParen) && close != NONE && close < r.end)
            .then_some(j)
    }

    /// `CREATE TABLE t (` with a column (or a constraint) a line.
    fn create_table(&mut self, r: Range<usize>, p: usize) -> Doc<'a> {
        let close = self.close[p];
        let head = self.expr(r.start..p);
        let (items, commas) = self.split_commas(p + 1..close);
        let mut inner = vec![Doc::Hard];
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                let comma = self.tok(commas[k - 1]);
                match self.options.commas {
                    CommaStyle::Trailing => inner.extend([comma, Doc::Hard]),
                    CommaStyle::Leading => inner.extend([Doc::Hard, comma, Doc::Space]),
                }
            }
            inner.push(self.expr(item));
        }
        let mut docs =
            vec![head, Doc::Space, self.tok(p), Doc::indent(inner), Doc::Hard, self.tok(close)];
        let rest = close + 1..r.end;
        if !rest.is_empty() {
            match self.as_query(rest.clone()) {
                Some(a) if a == rest.start => {
                    docs.extend([Doc::Space, self.tok(a), Doc::Hard]);
                    docs.push(self.query(a + 1..r.end, Sep::Hard));
                }
                _ => docs.extend([Doc::Space, self.inline(rest)]),
            }
        }
        Doc::seq(docs)
    }

    /// An `AS` at the top level followed by a query (`CREATE VIEW v AS SELECT`).
    fn as_query(&self, r: Range<usize>) -> Option<usize> {
        let mut j = r.start;
        while j < r.end {
            if self.name_free(j) == Some("AS")
                && j + 1 < r.end
                && (matches!(self.name_free(j + 1), Some("SELECT" | "WITH" | "VALUES"))
                    || self.paren_is_query(j + 1))
            {
                return Some(j);
            }
            j = self.skip(j, r.end);
        }
        None
    }

    /// Any other statement: on its line, broken after its commas when too long.
    fn inline(&mut self, r: Range<usize>) -> Doc<'a> {
        let (items, commas) = self.split_commas(r.clone());
        if commas.is_empty() {
            return self.expr(r);
        }
        let mut items = items.into_iter();
        let first = items.next().map(|i| self.expr(i)).unwrap_or_else(|| Doc::seq(Vec::new()));
        let mut rest = Vec::new();
        for (k, item) in items.enumerate() {
            self.separator(&mut rest, commas[k], Doc::Line);
            rest.push(self.expr(item));
        }
        Doc::group(vec![first, Doc::indent(rest)])
    }

    /// A comma between items, its break after it (or before it, commas leading).
    pub(super) fn separator(&self, docs: &mut Vec<Doc<'a>>, comma: usize, line: Doc<'a>) {
        // After a line comment the comma starts the next line: the item follows it there.
        let after_comment = comma
            .checked_sub(1)
            .is_some_and(|p| self.sig[p].trailing.last().is_some_and(|n| n.line));
        let comma = self.tok(comma);
        match self.options.commas {
            CommaStyle::Trailing if after_comment => docs.extend([comma, Doc::Space]),
            CommaStyle::Trailing => docs.extend([comma, line]),
            CommaStyle::Leading => docs.extend([line, comma, Doc::Space]),
        }
    }
}
