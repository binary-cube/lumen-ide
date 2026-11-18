//! The levels of a statement: parentheses and subqueries opened and closed, a FROM's or a CTE's
//! items finished.

use super::*;

impl<'a> Scan<'a> {
    /// `table` joins the level's tables (the target of its statement, the one joined).
    pub(super) fn add_table(&mut self, l: usize, table: ScopeTable) {
        let index = self.levels[l].tables.len();
        self.levels[l].tables.push(table);
        let st = self.state(l);
        st.from = FromState::AfterTable;
        match st.clause {
            Clause::Join => st.join = Some(index),
            Clause::Insert | Clause::Update | Clause::Merge => st.target = Some(index),
            Clause::Delete => st.target = Some(index),
            _ => {}
        }
    }

    /// A `UNION` / `INTERSECT` / `EXCEPT`: the next branch has its own tables.
    pub(super) fn branch(&mut self, l: usize) {
        let level = &self.levels[l];
        let next = Level {
            parent: level.parent,
            kind: level.kind,
            tables: Vec::new(),
            ctes: Vec::new(),
            outputs: Vec::new(),
            aliases: Vec::new(),
            first: level.first,
            open: level.open,
            before: level.before,
            using: false,
            state: State {
                clause: Clause::SetOperation,
                statement: level.state.statement,
                ..State::default()
            },
        };
        self.levels.push(next);
        let new = self.levels.len() - 1;
        if let Some(frame) = self.frames.iter_mut().rev().find(|f| matches!(f, Frame::Level(_))) {
            *frame = Frame::Level(new);
        }
    }

    /// The select list's item from the level's `item` to `end` (a comma, a clause, the
    /// level's end), as one of its outputs.
    pub(super) fn finish_item(&mut self, l: usize, end: usize) {
        let st = &self.levels[l].state;
        if st.clause != Clause::Select || !st.collecting || st.item >= end {
            return;
        }
        let start = st.item;
        let items: Vec<usize> =
            (start..end.min(self.sig.len())).filter(|&k| k != self.hole).collect();
        self.levels[l].state.item = end;
        let Some(&last) = items.last() else { return };
        let first = self.levels[l].first;
        let as_at = items.len().checked_sub(2).map(|k| items[k]);
        let output = if as_at.is_some_and(|a| self.is(a, &["AS"])) {
            let alias = self.name_at(last);
            if let Some(alias) = &alias {
                self.levels[first].aliases.push(alias.clone());
            }
            alias.map(OutputColumn::Named)
        } else if items.iter().all(|&k| {
            matches!(self.kind(k), Some(TokenKind::Word | TokenKind::QuotedIdent | TokenKind::Dot))
                || self.sig[k].text(self.text) == "*"
        }) {
            // `name`, `t.name`, `*`, `t.*`.
            let star = self.sig[last].text(self.text) == "*";
            if star {
                let qualifier =
                    (items.len() >= 3).then(|| self.name_at(items[items.len() - 3])).flatten();
                Some(OutputColumn::Star { qualifier, tables: Vec::new() })
            } else if items.len() == 2 && self.kind(items[0]) != Some(TokenKind::Dot) {
                // `expression alias` of two words (`name n`).
                let alias = self.name_at(last);
                if let Some(alias) = &alias {
                    self.levels[first].aliases.push(alias.clone());
                }
                alias.map(OutputColumn::Named)
            } else {
                self.name_at(last).map(OutputColumn::Named)
            }
        } else if items.len() >= 2
            && self.sig[last].is_name()
            && !matches!(
                self.kind(items[items.len() - 2]),
                Some(TokenKind::Dot | TokenKind::Operator | TokenKind::Cast)
            )
            && !(self.sig[last].kind == TokenKind::Word
                && CONTINUES.iter().chain(NOT_ALIAS).any(|w| self.sig[last].is_word(self.text, w)))
        {
            // `count(*) total`: a bare alias.
            let alias = self.name_at(last);
            if let Some(alias) = &alias {
                self.levels[first].aliases.push(alias.clone());
            }
            alias.map(OutputColumn::Named)
        } else {
            None
        };
        if let Some(output) = output {
            self.levels[first].outputs.push(output);
        }
    }

    pub(super) fn open(&mut self, i: usize) -> usize {
        let (l, group) = self.place();
        let next = i + 1;
        let query = next != self.hole && self.is(next, &["SELECT", "WITH"]);
        let st = self.levels[l].state.clone();
        let own = matches!(group, None | Some(Group::From));
        if query {
            let kind = if own && st.clause == Clause::With && st.cte == CteState::ExpectBody {
                LevelKind::Cte
            } else if own
                && matches!(st.clause, Clause::From | Clause::Join)
                && st.from == FromState::ExpectTable
            {
                LevelKind::Derived { lateral: st.lateral }
            } else {
                LevelKind::Nested
            };
            if kind == LevelKind::Cte {
                let index = self.levels[l].ctes.len().saturating_sub(1);
                self.pending_cte.push((l, index));
            }
            self.state(l).lateral = false;
            let new = self.levels.len();
            self.levels.push(Level {
                parent: Some(l),
                kind,
                tables: Vec::new(),
                ctes: Vec::new(),
                outputs: Vec::new(),
                aliases: Vec::new(),
                first: new,
                open: self.sig[i].start,
                before: self.levels[l].tables.len(),
                using: false,
                state: State { collecting: true, ..State::default() },
            });
            self.frames.push(Frame::Level(new));
            return i + 1;
        }
        let prev_name = i.checked_sub(1).filter(|&p| p != self.hole).is_some_and(|p| {
            let t = self.sig[p];
            t.kind == TokenKind::QuotedIdent
                || (t.kind == TokenKind::Word
                    && !CONTINUES.iter().chain(NOT_ALIAS).any(|w| t.is_word(self.text, w)))
        });
        let kind = if !own {
            if prev_name { Group::Call } else { Group::Expr }
        } else if st.clause == Clause::Insert
            && let Some(target) = st.target
            && matches!(st.from, FromState::AfterTable | FromState::AfterAlias)
        {
            Group::InsertColumns(target)
        } else if st.clause == Clause::With && st.cte == CteState::AfterName {
            Group::CteColumns
        } else if st.clause == Clause::Using {
            Group::Using
        } else if st.clause == Clause::Values {
            Group::Values
        } else if st.clause == Clause::Conflict
            && let Some(target) = st.target
        {
            Group::TableColumns(target)
        } else if st.ddl == Ddl::Index
            && st.from == FromState::AfterTable
            && let Some(last) = self.levels[l].tables.len().checked_sub(1)
        {
            Group::TableColumns(last)
        } else if st.ddl == Ddl::Create {
            Group::Definitions
        } else if matches!(st.clause, Clause::From | Clause::Join)
            && st.from == FromState::AfterTable
            && prev_name
        {
            // A table-valued function: `generate_series(1, 3) g`.
            if let Some(table) = self.levels[l].tables.last_mut()
                && let TableSource::Table(t) = &table.source
            {
                table.source = TableSource::Function(t.name.clone());
            }
            Group::Call
        } else if matches!(st.clause, Clause::From | Clause::Join)
            && st.from == FromState::ExpectTable
        {
            Group::From
        } else if prev_name {
            Group::Call
        } else {
            Group::Expr
        };
        self.frames.push(Frame::Group(kind));
        i + 1
    }

    pub(super) fn close(&mut self, i: usize) -> usize {
        // A `)` with nothing open: stray, ignored.
        if self.frames.len() <= 1 {
            return i + 1;
        }
        let Some(frame) = self.frames.pop() else { return i + 1 };
        match frame {
            Frame::Level(l) => {
                self.finish_item(l, i);
                self.ended(l, i);
            }
            Frame::Group(group) => {
                let (l, _) = self.place();
                let st = self.state(l);
                match group {
                    Group::InsertColumns(_) => st.from = FromState::AfterColumns,
                    Group::CteColumns => {}
                    Group::Values => st.values_done = true,
                    _ => {}
                }
            }
        }
        i + 1
    }

    /// Level `l` ended (its `)` read, or the statement's end at `end`).
    pub(super) fn ended(&mut self, l: usize, end: usize) {
        let first = self.levels[l].first;
        // Its `*`s mean its first branch's tables (those `t.*` names).
        let own: Vec<ScopeTable> = self.levels[first].tables.clone();
        for output in &mut self.levels[first].outputs {
            if let OutputColumn::Star { qualifier, tables } = output
                && tables.is_empty()
            {
                *tables = own
                    .iter()
                    .filter(|t| {
                        qualifier.as_ref().is_none_or(|q| t.answers_to(std::slice::from_ref(q)))
                    })
                    .filter_map(|t| t.table().cloned())
                    .collect();
            }
        }
        let Some(parent) = self.levels[l].parent else { return };
        let outputs = self.levels[first].outputs.clone();
        match self.levels[l].kind {
            LevelKind::Derived { .. } => {
                let start = self.levels[l].open;
                let table = ScopeTable {
                    source: TableSource::Derived { columns: outputs },
                    alias: None,
                    level: 0,
                    range: start..self.sig.get(end).map_or(start, |t| t.end),
                };
                self.levels[parent].tables.push(table);
                let last = self.levels[parent].tables.len() - 1;
                let st = self.state(parent);
                st.from = FromState::AfterTable;
                if st.clause == Clause::Join {
                    st.join = Some(last);
                }
            }
            LevelKind::Cte => {
                if let Some((owner, index)) = self.pending_cte.pop()
                    && let Some(cte) = self.levels[owner].ctes.get_mut(index)
                    && cte.columns.is_empty()
                {
                    cte.columns = outputs;
                }
                self.state(parent).cte = CteState::AfterBody;
            }
            LevelKind::Nested | LevelKind::Top => {}
        }
    }
}
