//! A query laid out: its clauses found and each on its line, their items broken when the
//! group doesn't fit, `WITH` and its CTEs.

use std::ops::Range;

use crate::sql::TokenKind;

use super::{Clause, Doc, Kind, Layout, ListStyle, MAX_DEPTH, NONE, Note, Sep, notes_after};

impl<'a, 'o> Layout<'a, 'o> {
    /// Whether the `(` at `i` holds a query.
    pub(super) fn paren_is_query(&self, i: usize) -> bool {
        let mut i = i;
        // `((SELECT …) UNION (SELECT …))`: the first inside tells.
        for _ in 0..MAX_DEPTH {
            if self.kind_at(i) != Some(TokenKind::OpenParen) || self.close[i] == NONE {
                return false;
            }
            match self.name_free(i + 1) {
                Some("SELECT" | "WITH" | "VALUES") => return true,
                _ if self.kind_at(i + 1) == Some(TokenKind::OpenParen) => i += 1,
                _ => return false,
            }
        }
        false
    }

    pub(super) fn query(&mut self, r: Range<usize>, sep: Sep) -> Doc<'a> {
        let clauses = self.clauses(r);
        let mut docs = Vec::with_capacity(clauses.len() * 2);
        for clause in clauses {
            if clause.kind == Kind::On && !docs.is_empty() {
                let ending = self.take_ending(&clause);
                let on = self.clause_doc(clause);
                let on = Doc::group(vec![Doc::indent(vec![Doc::Line, on])]);
                docs.push(notes_after(on, ending));
                continue;
            }
            if !docs.is_empty() {
                docs.push(sep.doc());
            }
            docs.push(self.clause(clause));
        }
        Doc::seq(docs)
    }

    pub(super) fn clauses(&self, r: Range<usize>) -> Vec<Clause> {
        let mut out: Vec<Clause> = Vec::new();
        let mut current: Option<(Kind, Range<usize>)> = None;
        let mut body_start = r.start;
        let mut j = r.start;
        while j < r.end {
            if let Some((kind, head_end)) = self.clause_at(j, r.start) {
                match current.take() {
                    Some((kind, head)) => out.push(Clause { kind, head, body: body_start..j }),
                    None if j > r.start => out.push(Clause {
                        kind: Kind::Lead,
                        head: r.start..r.start,
                        body: r.start..j,
                    }),
                    None => {}
                }
                let head_end = head_end.min(r.end);
                current = Some((kind, j..head_end));
                body_start = head_end;
                j = head_end;
                continue;
            }
            j = self.skip(j, r.end);
        }
        match current {
            Some((kind, head)) => out.push(Clause { kind, head, body: body_start..r.end }),
            None => out.push(Clause { kind: Kind::Lead, head: r.start..r.start, body: r }),
        }
        out
    }

    /// Whether a clause starts at `j` (in a query starting at `start`): its kind and where
    /// its head (its keywords) ends.
    fn clause_at(&self, j: usize, start: usize) -> Option<(Kind, usize)> {
        let word = self.name_free(j)?;
        let prev = if j > start { self.prev_upper(j) } else { None };
        let next = self.name_free(j + 1);
        let next_paren = self.kind_at(j + 1) == Some(TokenKind::OpenParen);
        let after = |words: &[&str]| prev.is_some_and(|p| words.contains(&p));
        let clause = match word {
            "SELECT" => (Kind::List, self.select_head(j)),
            "FROM" => {
                // `a IS [NOT] DISTINCT FROM b` is a comparison.
                let distinct = prev == Some("DISTINCT")
                    && matches!(self.prev_upper(j - 1), Some("IS" | "NOT"));
                if distinct || after(&["DELETE"]) {
                    return None;
                }
                (Kind::List, j + 1)
            }
            "WHERE" | "HAVING" | "QUALIFY" => (Kind::Cond, j + 1),
            "GROUP" | "ORDER" if next == Some("BY") => (Kind::List, j + 2),
            "WINDOW" => (Kind::List, j + 1),
            "LIMIT" | "OFFSET" | "FETCH" => (Kind::Plain, j + 1),
            "FOR" if matches!(next, Some("UPDATE" | "SHARE" | "NO" | "KEY")) => {
                (Kind::Plain, j + 1)
            }
            "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => {
                let all = matches!(next, Some("ALL" | "DISTINCT"));
                (Kind::SetOp, j + 1 + usize::from(all))
            }
            "INSERT"
                if !after(&["THEN", "DO", "ON", "OR", "FOR", "INSTEAD", "BEFORE", "AFTER"]) =>
            {
                let mut end = j + 1;
                if next == Some("OR") {
                    end += 2;
                }
                if self.name_free(end) == Some("INTO") {
                    end += 1;
                }
                (Kind::Plain, end)
            }
            "REPLACE" if next == Some("INTO") && !after(&["THEN", "OR"]) => (Kind::Plain, j + 2),
            "MERGE" => (Kind::Plain, j + 1 + usize::from(next == Some("INTO"))),
            "UPDATE"
                if !after(&[
                    "DO", "THEN", "FOR", "KEY", "OF", "ON", "OR", "INSTEAD", "BEFORE", "AFTER",
                ]) =>
            {
                (Kind::Plain, j + 1 + if next == Some("OR") { 2 } else { 0 })
            }
            "DELETE" if !after(&["THEN", "ON", "OR", "INSTEAD", "BEFORE", "AFTER"]) => {
                (Kind::Plain, j + 1 + usize::from(next == Some("FROM")))
            }
            "SET" => (Kind::List, j + 1),
            "VALUES" if !after(&["DEFAULT"]) => (Kind::List, j + 1),
            "RETURNING" => (Kind::List, j + 1),
            "INTO" if !after(&["INSERT", "REPLACE", "MERGE"]) => (Kind::Plain, j + 1),
            "USING" if !next_paren => (Kind::Plain, j + 1),
            "WHEN" => (Kind::Plain, j + 1),
            "ON" => match next {
                Some("CONFLICT") => (Kind::Plain, j + 2),
                Some("DUPLICATE") => (Kind::List, j + 4),
                _ => (Kind::On, j + 1),
            },
            "WITH" if j == start => (Kind::With, j + 1 + usize::from(next == Some("RECURSIVE"))),
            "JOIN" | "STRAIGHT_JOIN" => (Kind::Plain, j + 1),
            "INNER" | "LEFT" | "RIGHT" | "FULL" | "CROSS" | "NATURAL" | "OUTER" | "SEMI"
            | "ANTI"
                if !next_paren =>
            {
                let mut k = j;
                while k < j + 5
                    && matches!(
                        self.name_free(k),
                        Some(
                            "INNER"
                                | "LEFT"
                                | "RIGHT"
                                | "FULL"
                                | "CROSS"
                                | "NATURAL"
                                | "OUTER"
                                | "SEMI"
                                | "ANTI"
                        )
                    )
                {
                    k += 1;
                }
                if self.name_free(k) != Some("JOIN") {
                    return None;
                }
                (Kind::Plain, k + 1)
            }
            _ => return None,
        };
        Some(clause)
    }

    /// `SELECT [ALL | DISTINCT [ON (…)]]`: where the head ends.
    fn select_head(&self, j: usize) -> usize {
        match self.name_free(j + 1) {
            Some("ALL") => j + 2,
            Some("DISTINCT") => {
                let on = j + 2;
                let paren = on + 1;
                if self.name_free(on) == Some("ON")
                    && self.kind_at(paren) == Some(TokenKind::OpenParen)
                    && self.close[paren] != NONE
                {
                    self.close[paren] + 1
                } else {
                    j + 2
                }
            }
            _ => j + 1,
        }
    }

    pub(super) fn clause(&mut self, clause: Clause) -> Doc<'a> {
        let ending = self.take_ending(&clause);
        let doc = self.clause_doc(clause);
        notes_after(doc, ending)
    }

    /// The comments after a clause's last token when a line comment is among them: they
    /// end its line after the clause, whose groups don't break for them.
    fn take_ending(&mut self, clause: &Clause) -> Vec<Note<'a>> {
        let last = if clause.body.is_empty() { clause.head.end } else { clause.body.end };
        last.checked_sub(1)
            .filter(|&l| self.sig[l].trailing.last().is_some_and(|n| n.line))
            .map(|l| std::mem::take(&mut self.sig[l].trailing))
            .unwrap_or_default()
    }

    fn clause_doc(&mut self, clause: Clause) -> Doc<'a> {
        let head = self.expr(clause.head.clone());
        let body = clause.body;
        if body.is_empty() {
            return head;
        }
        match clause.kind {
            Kind::Lead => self.expr(body),
            Kind::List => {
                let (items, commas) = self.split_commas(body);
                let many = items.len() > 1;
                let mut inner = vec![Doc::Line];
                inner.extend(self.items(items, &commas));
                let list = vec![Doc::indent(inner)];
                let list = if many && self.options.lists == ListStyle::OnePerLine {
                    Doc::broken(list)
                } else {
                    Doc::group(list)
                };
                Doc::seq(vec![head, list])
            }
            Kind::Cond | Kind::On => Doc::seq(vec![head, Doc::Space, self.cond(body)]),
            Kind::Plain | Kind::SetOp => Doc::seq(vec![head, Doc::Space, self.expr(body)]),
            Kind::With => self.with(head, body),
        }
    }

    /// Items between their commas, each broken onto its line when the group breaks.
    pub(super) fn items(&mut self, items: Vec<Range<usize>>, commas: &[usize]) -> Vec<Doc<'a>> {
        let mut docs = Vec::with_capacity(items.len() * 3);
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                self.separator(&mut docs, commas[k - 1], Doc::Line);
            }
            docs.push(self.expr(item));
        }
        docs
    }

    /// `WITH a AS (…), b AS (…)`: each CTE's body indented under it.
    pub(super) fn with(&mut self, head: Doc<'a>, body: Range<usize>) -> Doc<'a> {
        let (items, commas) = self.split_commas(body);
        let mut docs = vec![head, Doc::Space];
        for (k, item) in items.into_iter().enumerate() {
            if k > 0 {
                self.separator(&mut docs, commas[k - 1], Doc::Hard);
            }
            docs.push(self.cte(item));
        }
        Doc::seq(docs)
    }

    pub(super) fn cte(&mut self, r: Range<usize>) -> Doc<'a> {
        let mut j = r.start;
        let mut as_at = None;
        while j < r.end {
            if self.name_free(j) == Some("AS") {
                as_at = Some(j);
                break;
            }
            j = self.skip(j, r.end);
        }
        let Some(a) = as_at else { return self.expr(r) };
        let mut p = a + 1;
        while matches!(self.name_free(p), Some("NOT" | "MATERIALIZED")) {
            p += 1;
        }
        if p >= r.end || !self.paren_is_query(p) || self.close[p] >= r.end {
            return self.expr(r);
        }
        let close = self.close[p];
        let mut docs = vec![
            self.expr(r.start..p),
            Doc::Space,
            self.tok(p),
            Doc::indent(vec![Doc::Hard, self.query(p + 1..close, Sep::Hard)]),
            Doc::Hard,
            self.tok(close),
        ];
        if close + 1 < r.end {
            docs.extend([Doc::Space, self.expr(close + 1..r.end)]);
        }
        Doc::seq(docs)
    }
}
