//! The walk over a statement's tokens: each one moves the scan along (its clause, its level, the
//! names it meets).

use super::*;

impl<'a> Scan<'a> {
    /// Notes what the name at `at` was read as (when names are kept).
    pub(super) fn note(&mut self, at: usize, level: usize, role: Role) {
        let ddl = self.levels[level].state.statement == Clause::Ddl;
        if let Some(seen) = &mut self.seen {
            seen.push(Seen { at, level, role, ddl });
        }
    }

    /// Notes the name at `at` as one of an expression of level `l`.
    pub(super) fn note_expr(&mut self, l: usize, at: usize, group: Option<Group>) {
        if self.seen.is_none() || !self.sig[at].is_name() {
            return;
        }
        let st = &self.levels[l].state;
        let role = Role::Expr {
            clause: st.clause,
            group,
            set_left: st.clause == Clause::Set && st.set_left,
            target: st.target,
        };
        self.note(at, l, role);
    }

    pub(super) fn word(&self, i: usize) -> Option<String> {
        let t = self.sig.get(i)?;
        (t.kind == TokenKind::Word).then(|| t.text(self.text).to_ascii_uppercase())
    }

    pub(super) fn is(&self, i: usize, words: &[&str]) -> bool {
        self.sig.get(i).is_some_and(|t| words.iter().any(|w| t.is_word(self.text, w)))
    }

    pub(super) fn kind(&self, i: usize) -> Option<TokenKind> {
        self.sig.get(i).map(|t| t.kind)
    }

    /// The level the reading is in (the innermost), and the group above it, if any.
    pub(super) fn place(&self) -> (usize, Option<Group>) {
        let mut group = None;
        for frame in self.frames.iter().rev() {
            match *frame {
                Frame::Level(l) => return (l, group),
                Frame::Group(g) => {
                    if group.is_none() {
                        group = Some(g);
                    }
                }
            }
        }
        (0, group)
    }

    pub(super) fn run(&mut self) {
        let mut i = 0;
        while i < self.sig.len() {
            if i == self.hole {
                let (level, group) = self.place();
                self.snapshot =
                    Some(Snapshot { level, state: self.levels[level].state.clone(), group });
                i += 1;
                continue;
            }
            i = match self.sig[i].kind {
                TokenKind::OpenParen => self.open(i),
                TokenKind::CloseParen => self.close(i),
                _ => self.token(i),
            };
        }
        // What's left open ends with the statement.
        while let Some(frame) = self.frames.pop() {
            if let Frame::Level(l) = frame {
                self.finish_item(l, self.sig.len());
                self.ended(l, self.sig.len());
            }
        }
    }

    /// The name at `i` as written (unquoted), unless it's the hole.
    pub(super) fn name_at(&self, i: usize) -> Option<String> {
        let t = self.sig.get(i)?;
        t.is_name().then(|| identifier(self.text, t).into_owned())
    }

    /// The qualified name starting at `i` (never across the hole) and the index after it.
    pub(super) fn qualified(&self, i: usize) -> Option<(Vec<String>, usize)> {
        let mut parts = vec![self.name_at(i)?];
        let mut j = i + 1;
        while self.kind(j) == Some(TokenKind::Dot)
            && j + 1 != self.hole
            && let Some(next) = self.name_at(j + 1)
        {
            parts.push(next);
            j += 2;
        }
        Some((parts, j))
    }

    pub(super) fn table_ref(parts: &[String]) -> TableRef {
        match parts {
            [.., schema, name] => TableRef::in_schema(schema.clone(), name.clone()),
            [name] => TableRef::new(name.clone()),
            [] => TableRef::new(String::new()),
        }
    }

    pub(super) fn state(&mut self, l: usize) -> &mut State {
        &mut self.levels[l].state
    }

    /// A token read at the level's own depth (or in a group that's the level's).
    pub(super) fn token(&mut self, i: usize) -> usize {
        let (l, group) = self.place();
        match group {
            None | Some(Group::From) => self.level_token(l, i),
            Some(Group::CteColumns) => {
                if let Some(name) = self.name_at(i)
                    && let Some(cte) = self.levels[l].ctes.last_mut()
                {
                    cte.columns.push(OutputColumn::Named(name));
                }
                i + 1
            }
            Some(Group::Definitions) => {
                // A column's `REFERENCES t (…)` in a table's definitions.
                if self.is(i, &["REFERENCES"]) {
                    let st = self.state(l);
                    st.from = FromState::ExpectTable;
                    return i + 1;
                }
                if self.levels[l].state.from == FromState::ExpectTable {
                    return self.name_token(l, i);
                }
                self.state(l).from = FromState::None;
                i + 1
            }
            Some(g) => {
                if !matches!(g, Group::CteColumns) {
                    self.note_expr(l, i, Some(g));
                }
                i + 1
            }
        }
    }

    pub(super) fn level_token(&mut self, l: usize, i: usize) -> usize {
        let t = self.sig[i];
        match t.kind {
            TokenKind::Comma => {
                let st = self.state(l);
                match st.clause {
                    Clause::Select => {
                        self.finish_item(l, i);
                        self.state(l).item = i + 1;
                    }
                    Clause::From | Clause::Join | Clause::On | Clause::Using
                        if st.from != FromState::None =>
                    {
                        st.clause = Clause::From;
                        st.from = FromState::ExpectTable;
                        st.join = None;
                    }
                    Clause::Delete if st.statement == Clause::Delete => {
                        st.from = FromState::ExpectTable;
                    }
                    Clause::With => st.cte = CteState::ExpectName,
                    Clause::Set => st.set_left = true,
                    _ => {}
                }
                return i + 1;
            }
            TokenKind::Operator if t.text(self.text) == "=" => {
                self.state(l).set_left = false;
                return i + 1;
            }
            TokenKind::Word => {}
            TokenKind::QuotedIdent => return self.name_token(l, i),
            _ => return i + 1,
        }
        let Some(w) = self.word(i) else { return i + 1 };
        let st = self.levels[l].state.clone();
        let prev = i.checked_sub(1).and_then(|p| self.word(p));
        match w.as_str() {
            "WITH" if matches!(st.clause, Clause::Start) => {
                let st = self.state(l);
                st.clause = Clause::With;
                st.cte = CteState::ExpectName;
            }
            "RECURSIVE" if st.clause == Clause::With => {}
            "AS" if st.clause == Clause::With && st.cte == CteState::AfterName => {
                self.state(l).cte = CteState::ExpectBody;
            }
            "MATERIALIZED" | "NOT" if st.clause == Clause::With => {}
            "SELECT" => {
                self.finish_item(l, i);
                let first = self.levels[l].outputs.is_empty() && self.levels[l].first == l;
                let st = self.state(l);
                if matches!(st.statement, Clause::Start) {
                    st.statement = Clause::Select;
                }
                st.clause = Clause::Select;
                st.item = i + 1;
                st.collecting = first;
                st.from = FromState::None;
            }
            "DISTINCT" | "ALL" if st.clause == Clause::Select && st.item == i => {
                self.state(l).item = i + 1;
            }
            "FROM" if prev.as_deref() == Some("DISTINCT") && st.clause != Clause::Select => {}
            "FROM" => {
                self.finish_item(l, i);
                let st = self.state(l);
                if (st.clause == Clause::Delete && st.target.is_none()) || st.clause == Clause::Ddl
                {
                    st.from = FromState::ExpectTable;
                } else {
                    st.clause = Clause::From;
                    st.from = FromState::ExpectTable;
                    st.join = None;
                }
            }
            "JOIN" | "STRAIGHT_JOIN" => {
                let st = self.state(l);
                st.clause = Clause::Join;
                st.from = FromState::ExpectTable;
                st.join = None;
            }
            "INNER" | "LEFT" | "RIGHT" | "FULL" | "OUTER" | "CROSS" | "NATURAL"
                if matches!(
                    st.clause,
                    Clause::From | Clause::Join | Clause::On | Clause::Using
                ) =>
            {
                if w == "NATURAL" {
                    self.levels[l].using = true;
                }
                let st = self.state(l);
                st.clause = Clause::From;
                st.from = FromState::JoinPrefix;
            }
            "LATERAL" => self.state(l).lateral = true,
            "ON" if st.clause == Clause::Select => {}
            "ON" if matches!(st.clause, Clause::From | Clause::Join)
                && matches!(st.from, FromState::AfterTable | FromState::AfterAlias) =>
            {
                let st = self.state(l);
                st.clause = Clause::On;
                st.from = FromState::None;
            }
            "ON" if st.ddl == Ddl::Index => {
                let st = self.state(l);
                st.from = FromState::ExpectTable;
            }
            "ON" if matches!(
                st.clause,
                Clause::Values | Clause::Insert | Clause::Select | Clause::From
            ) || st.statement == Clause::Insert =>
            {
                // `ON CONFLICT` (SQLite, PostgreSQL), `ON DUPLICATE KEY` (MySQL).
                let st = self.state(l);
                st.clause = Clause::Conflict;
                st.from = FromState::None;
            }
            "USING" if st.statement == Clause::Delete && st.clause == Clause::Delete => {
                let st = self.state(l);
                st.clause = Clause::From;
                st.from = FromState::ExpectTable;
            }
            "USING" if st.clause == Clause::Merge => {
                let st = self.state(l);
                st.clause = Clause::Join;
                st.from = FromState::ExpectTable;
                st.join = None;
            }
            "USING" if matches!(st.clause, Clause::From | Clause::Join) => {
                self.levels[l].using = true;
                let st = self.state(l);
                st.clause = Clause::Using;
                st.from = FromState::AfterAlias;
            }
            "WHERE" => {
                self.finish_item(l, i);
                let st = self.state(l);
                st.clause = Clause::Where;
                st.from = FromState::None;
            }
            "GROUP" => {
                self.finish_item(l, i);
                self.state(l).by = Some(Clause::GroupBy);
            }
            "ORDER" => {
                self.finish_item(l, i);
                self.state(l).by = Some(Clause::OrderBy);
            }
            "BY" if st.by.is_some() => {
                let st = self.state(l);
                st.clause = st.by.take().unwrap_or(Clause::Other);
                st.from = FromState::None;
            }
            "HAVING" => {
                self.finish_item(l, i);
                self.state(l).clause = Clause::Having;
            }
            "WINDOW" => {
                self.finish_item(l, i);
                self.state(l).clause = Clause::Window;
            }
            "LIMIT" | "OFFSET" | "FETCH" => {
                self.finish_item(l, i);
                let st = self.state(l);
                st.clause = Clause::Limit;
                st.from = FromState::None;
            }
            "FOR" if st.clause != Clause::Ddl => {
                self.finish_item(l, i);
                self.state(l).clause = Clause::Other;
            }
            "UNION" | "INTERSECT" | "EXCEPT" => {
                self.finish_item(l, i);
                self.branch(l);
            }
            "RETURNING" => {
                self.finish_item(l, i);
                let st = self.state(l);
                st.clause = Clause::Returning;
                st.from = FromState::None;
            }
            "SET"
                if matches!(st.statement, Clause::Update | Clause::Merge)
                    || st.clause == Clause::Conflict
                    || st.clause == Clause::Update =>
            {
                let st = self.state(l);
                st.clause = Clause::Set;
                st.set_left = true;
                st.from = FromState::None;
            }
            "SET" if matches!(st.clause, Clause::Start) => {
                let st = self.state(l);
                st.clause = Clause::Other;
                st.statement = Clause::Other;
            }
            "SEARCH_PATH" if st.statement == Clause::Other => self.state(l).search_path = true,
            "VALUES" => {
                let st = self.state(l);
                st.clause = Clause::Values;
                st.from = FromState::None;
                st.values_done = false;
            }
            "INTO" if st.clause == Clause::Insert => {
                let st = self.state(l);
                st.from = FromState::ExpectTable;
            }
            "INTO" if st.clause == Clause::Merge => {
                let st = self.state(l);
                st.from = FromState::ExpectTable;
            }
            "INTO" if st.clause == Clause::Select => {
                self.finish_item(l, i);
                self.state(l).clause = Clause::Into;
            }
            "INSERT" | "REPLACE" if st.statement == Clause::Merge => {
                // `MERGE … WHEN NOT MATCHED THEN INSERT (…)`: the target's columns.
                let st = self.state(l);
                st.clause = Clause::Insert;
                st.from = FromState::AfterTable;
            }
            "INSERT" | "REPLACE"
                if matches!(st.clause, Clause::Start | Clause::With)
                    && matches!(st.statement, Clause::Start) =>
            {
                let st = self.state(l);
                st.clause = Clause::Insert;
                st.statement = Clause::Insert;
                st.from = FromState::None;
            }
            "UPDATE" if st.statement == Clause::Merge || st.clause == Clause::Conflict => {}
            "UPDATE"
                if matches!(st.clause, Clause::Start | Clause::With)
                    && matches!(st.statement, Clause::Start) =>
            {
                let st = self.state(l);
                st.clause = Clause::Update;
                st.statement = Clause::Update;
                st.from = FromState::ExpectTable;
            }
            "DELETE" if st.statement == Clause::Merge => {}
            "DELETE" if matches!(st.clause, Clause::Start | Clause::With) => {
                let st = self.state(l);
                st.clause = Clause::Delete;
                st.statement = Clause::Delete;
                st.from = FromState::None;
            }
            "MERGE" if matches!(st.clause, Clause::Start | Clause::With) => {
                let st = self.state(l);
                st.clause = Clause::Merge;
                st.statement = Clause::Merge;
                st.from = FromState::None;
            }
            "WHEN" if st.statement == Clause::Merge && st.clause != Clause::Select => {
                let st = self.state(l);
                st.clause = Clause::On;
                st.from = FromState::None;
            }
            "EXPLAIN" | "ANALYZE" | "VERBOSE" | "QUERY" | "PLAN"
                if matches!(st.clause, Clause::Start) && st.ddl == Ddl::None =>
            {
                if w == "ANALYZE"
                    && !self.is(i + 1, &["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "VERBOSE"])
                    && prev.is_none()
                {
                    let st = self.state(l);
                    st.clause = Clause::Ddl;
                    st.ddl = Ddl::Existing;
                    st.from = FromState::ExpectTable;
                }
            }
            "TABLE" if matches!(st.clause, Clause::Start) => {
                let st = self.state(l);
                st.clause = Clause::Ddl;
                st.statement = Clause::Select;
                st.ddl = Ddl::Existing;
                st.from = FromState::ExpectTable;
            }
            "CREATE" if matches!(st.clause, Clause::Start) => {
                let st = self.state(l);
                st.clause = Clause::Ddl;
                st.statement = Clause::Ddl;
                st.ddl = Ddl::Create;
            }
            "INDEX" if st.ddl == Ddl::Create => self.state(l).ddl = Ddl::Index,
            "DROP" | "ALTER" | "TRUNCATE" | "VACUUM" | "REINDEX" | "LOCK" | "COMMENT"
                if matches!(st.clause, Clause::Start) =>
            {
                let st = self.state(l);
                st.clause = Clause::Ddl;
                st.statement = Clause::Ddl;
                st.ddl = Ddl::Existing;
                if matches!(w.as_str(), "TRUNCATE" | "VACUUM" | "LOCK") {
                    st.from = FromState::ExpectTable;
                }
            }
            "TABLE" | "VIEW" if st.ddl == Ddl::Existing => {
                self.state(l).from = FromState::ExpectTable;
            }
            "IF" | "EXISTS" | "ONLY" | "MATERIALIZED" | "UNIQUE" | "CONCURRENTLY" | "ANALYZE"
            | "VERBOSE" | "FULL" | "FREEZE" | "TEMP" | "TEMPORARY"
                if st.clause == Clause::Ddl => {}
            "ONLY" if st.from == FromState::ExpectTable => {}
            "REFERENCES" => {
                let st = self.state(l);
                st.from = FromState::ExpectTable;
                st.ddl = Ddl::Existing;
            }
            "AS" if matches!(st.from, FromState::AfterTable) => {
                self.state(l).from = FromState::ExpectAlias;
            }
            "BEGIN" | "COMMIT" | "ROLLBACK" | "END" | "PRAGMA" | "SHOW" | "SAVEPOINT"
            | "RELEASE" | "ATTACH" | "DETACH" | "GRANT" | "REVOKE" | "COPY" | "CALL" | "DO"
                if matches!(st.clause, Clause::Start) =>
            {
                let st = self.state(l);
                st.clause = Clause::Other;
                st.statement = Clause::Other;
            }
            _ => return self.name_token(l, i),
        }
        i + 1
    }

    /// A name read at the level: a table where one is expected, its alias, a CTE's name.
    pub(super) fn name_token(&mut self, l: usize, i: usize) -> usize {
        let st = self.levels[l].state.clone();
        if st.clause == Clause::With && st.cte == CteState::ExpectName {
            if let Some(name) = self.name_at(i) {
                let t = self.sig[i];
                self.levels[l].ctes.push(Cte { name, columns: Vec::new(), range: t.range() });
                self.state(l).cte = CteState::AfterName;
                self.note(i, l, Role::Cte);
            }
            return i + 1;
        }
        match st.from {
            FromState::ExpectTable => {
                let Some((parts, next)) = self.qualified(i) else { return i + 1 };
                // `schema.|`: the name is being typed, its schema is the qualifier.
                if self.kind(next) == Some(TokenKind::Dot) && next + 1 == self.hole {
                    return next + 1;
                }
                let range = self.sig[i].start..self.sig[next - 1].end;
                let table = ScopeTable {
                    source: TableSource::Table(Self::table_ref(&parts)),
                    alias: None,
                    level: 0,
                    range,
                };
                self.add_table(l, table);
                let index = self.levels[l].tables.len() - 1;
                for k in (i..next).step_by(2) {
                    let role = if k + 1 == next { Role::Table(index) } else { Role::Schema(index) };
                    self.note(k, l, role);
                }
                return next;
            }
            FromState::AfterTable | FromState::ExpectAlias => {
                let t = self.sig[i];
                let keyword =
                    t.kind == TokenKind::Word && NOT_ALIAS.iter().any(|w| t.is_word(self.text, w));
                if (!keyword || st.from == FromState::ExpectAlias)
                    && let Some(alias) = self.name_at(i)
                {
                    if let Some(table) = self.levels[l].tables.last_mut() {
                        table.alias = Some(alias);
                    }
                    self.state(l).from = FromState::AfterAlias;
                    if let Some(index) = self.levels[l].tables.len().checked_sub(1) {
                        self.note(i, l, Role::Alias(index));
                    }
                }
            }
            _ => {
                let (_, group) = self.place();
                self.note_expr(l, i, group);
            }
        }
        i + 1
    }
}
