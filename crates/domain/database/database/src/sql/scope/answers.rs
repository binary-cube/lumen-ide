//! What the scan answers: the tables and CTEs in scope, the names resolved, the role of an
//! expression, what may come at the caret.

use super::*;

impl<'a> Scan<'a> {
    /// The tables visible from level `l`: its own (of its branch), then, as its kind lets
    /// it see out, the outer ones'.
    pub(super) fn visible(&self, l: usize) -> Vec<ScopeTable> {
        let mut out = Vec::new();
        let mut level = Some(l);
        let mut depth = 0;
        // How many of the level's tables it sees (a `LATERAL` child: those before it).
        let mut seen = usize::MAX;
        while let Some(at) = level {
            let lv = &self.levels[at];
            for t in lv.tables.iter().take(seen) {
                let mut t = t.clone();
                t.level = depth;
                out.push(t);
            }
            (level, seen) = match lv.kind {
                LevelKind::Nested => (lv.parent, usize::MAX),
                LevelKind::Derived { lateral: true } => (lv.parent, lv.before),
                _ => (None, 0),
            };
            depth += 1;
        }
        out
    }

    /// The CTEs visible from level `l` (its own and every outer level's).
    pub(super) fn ctes(&self, l: usize) -> Vec<Cte> {
        let mut out: Vec<Cte> = Vec::new();
        let mut level = Some(l);
        while let Some(at) = level {
            let lv = &self.levels[at];
            for c in self.levels[lv.first].ctes.iter().chain(&lv.ctes) {
                if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&c.name)) {
                    out.push(c.clone());
                }
            }
            level = lv.parent;
        }
        out
    }

    /// The tables visible from level `l` (a CTE's name being the CTE, unless qualified with
    /// a schema), and its CTEs.
    pub(super) fn scope_tables(&self, l: usize) -> (Vec<ScopeTable>, Vec<Cte>) {
        let ctes = self.ctes(l);
        let mut tables = self.visible(l);
        for t in &mut tables {
            if let TableSource::Table(r) = &t.source
                && r.schema.is_none()
                && let Some(cte) = ctes.iter().find(|c| c.name.eq_ignore_ascii_case(&r.name))
            {
                t.source =
                    TableSource::Cte { name: cte.name.clone(), columns: cte.columns.clone() };
            }
        }
        (tables, ctes)
    }

    /// Every name read, with what it is and the scopes it's read in.
    pub(super) fn resolution(&self) -> Resolution {
        let scopes = (0..self.levels.len())
            .map(|l| {
                let (tables, ctes) = self.scope_tables(l);
                let level = &self.levels[l];
                LevelScope {
                    tables,
                    own: level.tables.len(),
                    ctes,
                    aliases: self.levels[level.first].aliases.clone(),
                    using: level.using,
                }
            })
            .collect();
        let mut names: Vec<Name> = self
            .seen
            .iter()
            .flatten()
            .filter_map(|seen| {
                let token = self.sig[seen.at];
                let clause = match seen.role {
                    Role::Expr { clause, .. } => clause,
                    _ => Clause::From,
                };
                let role = match seen.role {
                    Role::Table(t) => NameRole::Table { table: t },
                    Role::Schema(t) => NameRole::Schema { table: t },
                    Role::Alias(t) => NameRole::Alias { table: t },
                    Role::Cte => NameRole::CteName,
                    Role::Expr { clause, group, set_left, target } => {
                        self.expression_role(seen.at, clause, group, set_left, target)?
                    }
                };
                Some(Name {
                    range: token.range(),
                    text: identifier(self.text, &token).into_owned(),
                    quoted: token.kind == TokenKind::QuotedIdent,
                    level: seen.level,
                    role,
                    clause,
                    ddl: seen.ddl,
                })
            })
            .collect();
        names.sort_by_key(|n| n.range.start);
        names.dedup_by_key(|n| n.range.start);
        Resolution { names, scopes }
    }

    /// What the name at `k`, read in an expression, is (`None`: a keyword, a new name).
    pub(super) fn expression_role(
        &self,
        k: usize,
        clause: Clause,
        group: Option<Group>,
        set_left: bool,
        target: Option<usize>,
    ) -> Option<NameRole> {
        let token = self.sig[k];
        let next = self.sig.get(k + 1).copied();
        let after = self.sig.get(k + 2).copied();
        let mut qualifier = Vec::new();
        let mut j = k;
        while j >= 2
            && self.kind(j - 1) == Some(TokenKind::Dot)
            && let Some(name) = self.name_at(j - 2)
        {
            qualifier.insert(0, name);
            j -= 2;
        }
        let bare = qualifier.is_empty();
        if bare
            && token.kind == TokenKind::Word
            && KEYWORDS.iter().any(|w| token.is_word(self.text, w))
        {
            return None;
        }
        // `AS total`: a new name.
        if bare && k.checked_sub(1).is_some_and(|p| self.sig[p].is_word(self.text, "AS")) {
            return None;
        }
        // `u.|name`, `s.t.|c`, `t.*`: a qualifier.
        if next.is_some_and(|n| n.kind == TokenKind::Dot)
            && after.is_some_and(|a| a.is_name() || a.text(self.text) == "*")
        {
            let mut parts = qualifier;
            parts.push(identifier(self.text, &token).into_owned());
            return Some(NameRole::Qualifier { parts });
        }
        if next.is_some_and(|n| n.kind == TokenKind::OpenParen) {
            return Some(NameRole::Function { qualifier });
        }
        match group {
            Some(Group::InsertColumns(t) | Group::TableColumns(t)) if bare => {
                return Some(NameRole::TargetColumn { table: t });
            }
            Some(Group::Values | Group::Definitions | Group::CteColumns) => return None,
            Some(Group::Using) => {
                return Some(NameRole::Column { qualifier, certain: false });
            }
            _ => {}
        }
        if set_left && bare && group.is_none() {
            return target.map(|table| NameRole::TargetColumn { table });
        }
        let expression = matches!(
            clause,
            Clause::Select
                | Clause::On
                | Clause::Where
                | Clause::GroupBy
                | Clause::Having
                | Clause::Window
                | Clause::OrderBy
                | Clause::Set
                | Clause::Returning
        );
        if !expression {
            return None;
        }
        let certain = !bare || self.operand_at(k, group);
        Some(NameRole::Column { qualifier, certain })
    }

    /// The bare name at `k` is surely an operand: where an expression starts (after a comma,
    /// a parenthesis, an operator, a word an expression follows), not a function, a typed
    /// literal (`DATE '…'`), a type (`::int`, `AS int`), an alias given (`SELECT a b`) or
    /// a part of a call's syntax (`EXTRACT(YEAR FROM d)`).
    pub(super) fn operand_at(&self, k: usize, group: Option<Group>) -> bool {
        let Some(prev) = k.checked_sub(1).map(|p| self.sig[p]) else { return false };
        let starts = match prev.kind {
            TokenKind::Comma | TokenKind::OpenParen => true,
            TokenKind::Operator => true,
            TokenKind::Word => OPERAND_AFTER.iter().any(|w| prev.is_word(self.text, w)),
            _ => false,
        };
        if !starts {
            return false;
        }
        match self.sig.get(k + 1) {
            Some(n) if matches!(n.kind, TokenKind::String(_)) => false,
            Some(n) if group == Some(Group::Call) && n.is_word(self.text, "FROM") => false,
            _ => true,
        }
    }

    pub(super) fn result(&self, offset: usize) -> QueryScope {
        let snap = self.snapshot.clone().unwrap_or(Snapshot {
            level: 0,
            state: self.levels[0].state.clone(),
            group: None,
        });
        let (tables, ctes) = self.scope_tables(snap.level);
        let mut named: Vec<(usize, TableRef)> = Vec::new();
        let all_ctes: Vec<&Cte> = self.levels.iter().flat_map(|lv| &lv.ctes).collect();
        for lv in &self.levels {
            for t in &lv.tables {
                if let TableSource::Table(r) = &t.source
                    && !(r.schema.is_none()
                        && all_ctes.iter().any(|c| c.name.eq_ignore_ascii_case(&r.name)))
                    && !named.iter().any(|(_, n)| n == r)
                {
                    named.push((t.range.start, r.clone()));
                }
            }
        }
        named.sort_by_key(|(at, _)| *at);
        let named = named.into_iter().map(|(_, r)| r).collect();
        let (qualifier, before) = self.qualifier_before_hole();
        let st = &snap.state;
        let clause = st.clause;
        let own = self.levels[snap.level].tables.len();
        let expect = self.expect(&snap, qualifier, before, own);
        // The select list's aliases: in ORDER BY; in GROUP BY / HAVING where allowed.
        let first = self.levels[snap.level].first;
        let aliases = match clause {
            Clause::OrderBy => self.levels[first].aliases.clone(),
            Clause::GroupBy if !matches!(self.flavor, Flavor::Generic) => {
                self.levels[first].aliases.clone()
            }
            Clause::Having if matches!(self.flavor, Flavor::Sqlite | Flavor::MySql) => {
                self.levels[first].aliases.clone()
            }
            _ => Vec::new(),
        };
        let star = self.star(&snap, offset);
        QueryScope {
            expect,
            replace: offset..offset,
            prefix: String::new(),
            quoted: false,
            clause,
            tables,
            ctes,
            aliases,
            star,
            value_of: None,
            named,
        }
    }

    /// The qualifier written right before the hole (`a.b.|` → `[a, b]`), and the index of
    /// the token before it (or before the hole when none).
    pub(super) fn qualifier_before_hole(&self) -> (Vec<String>, Option<usize>) {
        let mut parts = Vec::new();
        let mut j = self.hole;
        while j >= 2
            && self.kind(j - 1) == Some(TokenKind::Dot)
            && let Some(name) = self.name_at(j - 2)
        {
            parts.insert(0, name);
            j -= 2;
        }
        (parts, j.checked_sub(1))
    }

    pub(super) fn expect(
        &self,
        snap: &Snapshot,
        qualifier: Vec<String>,
        before: Option<usize>,
        own: usize,
    ) -> Expect {
        let st = &snap.state;
        let prev = before.map(|b| self.sig[b]);
        let prev_word = before.and_then(|b| self.word(b));
        let qualified = !qualifier.is_empty();
        match snap.group {
            Some(Group::InsertColumns(t)) => {
                return if qualified {
                    Expect::Nothing
                } else {
                    Expect::InsertColumns { table: t }
                };
            }
            Some(Group::TableColumns(t)) => return Expect::TableColumns { table: t },
            Some(Group::Using) => {
                let right = st.join.unwrap_or(own.saturating_sub(1));
                return Expect::UsingColumns { right, left: (0..right).collect() };
            }
            Some(Group::Definitions) if st.from == FromState::ExpectTable => {
                return Expect::Table { schema: qualifier.last().cloned(), join: false };
            }
            Some(Group::CteColumns | Group::Definitions) => return Expect::Nothing,
            Some(Group::Values) => {
                return self.expression(qualifier, before, prev, prev_word, Clause::Values);
            }
            Some(Group::Expr | Group::Call) => {
                return self.expression(qualifier, before, prev, prev_word, st.clause);
            }
            Some(Group::From) | None => {}
        }
        if st.search_path && !qualified {
            return Expect::Schema;
        }
        if st.clause == Clause::With {
            return match st.cte {
                CteState::ExpectName | CteState::AfterName | CteState::ExpectBody => {
                    Expect::Nothing
                }
                CteState::AfterBody => Expect::Keyword(KeywordPlace::AfterCte),
                CteState::None => Expect::Keyword(KeywordPlace::Start),
            };
        }
        if let Some(by) = st.by
            && matches!(by, Clause::GroupBy | Clause::OrderBy)
            && matches!(prev_word.as_deref(), Some("GROUP" | "ORDER"))
        {
            return Expect::Keyword(KeywordPlace::By);
        }
        // Where a table is named.
        match st.from {
            FromState::ExpectTable => {
                if st.ddl == Ddl::Create {
                    return Expect::Nothing;
                }
                return Expect::Table {
                    schema: qualifier.last().cloned(),
                    join: st.clause == Clause::Join,
                };
            }
            FromState::ExpectAlias => return Expect::Nothing,
            FromState::JoinPrefix => return Expect::Keyword(KeywordPlace::JoinWord),
            FromState::AfterTable | FromState::AfterAlias
                if matches!(st.clause, Clause::From | Clause::Join | Clause::Delete)
                    && !qualified =>
            {
                return Expect::Keyword(KeywordPlace::AfterTable {
                    join: st.clause == Clause::Join,
                });
            }
            FromState::AfterTable | FromState::AfterAlias | FromState::AfterColumns
                if st.clause == Clause::Insert && !qualified =>
            {
                return Expect::Keyword(KeywordPlace::AfterInsertTarget);
            }
            FromState::AfterTable | FromState::AfterAlias
                if st.clause == Clause::Update && !qualified =>
            {
                return Expect::Keyword(KeywordPlace::AfterUpdateTarget);
            }
            FromState::AfterTable | FromState::AfterAlias if st.clause == Clause::Merge => {
                return Expect::Keyword(KeywordPlace::Other);
            }
            _ => {}
        }
        match st.clause {
            Clause::Start => Expect::Keyword(KeywordPlace::Start),
            Clause::Insert => {
                if st.from == FromState::None {
                    Expect::Keyword(KeywordPlace::Into)
                } else {
                    Expect::Keyword(KeywordPlace::AfterInsertTarget)
                }
            }
            Clause::Delete if st.target.is_none() => Expect::Keyword(KeywordPlace::From),
            Clause::Merge => Expect::Keyword(KeywordPlace::Into),
            Clause::On if matches!(prev_word.as_deref(), Some("ON")) && !qualified => {
                match st.join {
                    Some(right) => Expect::JoinCondition { right, left: (0..right).collect() },
                    None => self.expression(qualifier, before, prev, prev_word, Clause::On),
                }
            }
            Clause::Set if st.set_left && !qualified => match st.target {
                Some(t)
                    if matches!(prev_word.as_deref(), Some("SET"))
                        || prev.is_some_and(|p| p.kind == TokenKind::Comma) =>
                {
                    Expect::TableColumns { table: t }
                }
                _ => Expect::Keyword(KeywordPlace::AfterSet),
            },
            Clause::Values => {
                if st.values_done {
                    Expect::Keyword(KeywordPlace::AfterValues)
                } else {
                    Expect::Value
                }
            }
            Clause::Limit => Expect::Value,
            Clause::Into => Expect::Nothing,
            Clause::SetOperation => Expect::Keyword(KeywordPlace::SetOperation),
            // `ALTER TABLE t DROP COLUMN |`, `RENAME COLUMN |`, `ALTER COLUMN |`.
            Clause::Ddl
                if prev_word.as_deref() == Some("COLUMN")
                    && own > 0
                    && before
                        .and_then(|b| b.checked_sub(1))
                        .is_some_and(|b| self.is(b, &["DROP", "ALTER", "RENAME"])) =>
            {
                Expect::TableColumns { table: 0 }
            }
            // `CREATE TABLE |`: a new name.
            Clause::Ddl
                if st.ddl == Ddl::Create
                    && matches!(
                        prev_word.as_deref(),
                        Some(
                            "TABLE"
                                | "VIEW"
                                | "INDEX"
                                | "EXISTS"
                                | "TRIGGER"
                                | "SCHEMA"
                                | "SEQUENCE"
                                | "TYPE"
                                | "FUNCTION"
                                | "PROCEDURE"
                                | "DATABASE"
                        )
                    ) =>
            {
                Expect::Nothing
            }
            Clause::Ddl => Expect::Keyword(KeywordPlace::Other),
            Clause::Other => Expect::Keyword(KeywordPlace::Other),
            clause => self.expression(qualifier, before, prev, prev_word, clause),
        }
    }

    /// In an expression of `clause`: a column where an operand goes, a keyword after one.
    pub(super) fn expression(
        &self,
        qualifier: Vec<String>,
        before: Option<usize>,
        prev: Option<Token>,
        prev_word: Option<String>,
        clause: Clause,
    ) -> Expect {
        if !qualifier.is_empty() {
            return Expect::Column { qualifier: Some(qualifier) };
        }
        let Some(prev) = prev else { return Expect::Column { qualifier: None } };
        let operand = match prev.kind {
            // `SELECT * |`, `t.* |`: a whole operand, not a product.
            TokenKind::Operator if prev.text(self.text) == "*" => !before.is_none_or(|b| {
                b.checked_sub(1).is_none_or(|p| {
                    let t = self.sig[p];
                    matches!(t.kind, TokenKind::Comma | TokenKind::Dot | TokenKind::OpenParen)
                        || ["SELECT", "DISTINCT", "ALL"].iter().any(|w| t.is_word(self.text, w))
                })
            }),
            TokenKind::Operator | TokenKind::Comma | TokenKind::OpenParen => true,
            TokenKind::Cast => return Expect::Nothing,
            TokenKind::Word => {
                let w = prev_word.unwrap_or_default();
                if w == "AS" {
                    return Expect::Nothing;
                }
                CONTINUES.contains(&w.as_str())
            }
            _ => false,
        };
        if operand {
            if clause == Clause::Values || clause == Clause::Limit {
                return Expect::Value;
            }
            return Expect::Column { qualifier: None };
        }
        Expect::Keyword(match clause {
            Clause::Select => KeywordPlace::AfterSelectItem,
            Clause::Where | Clause::On | Clause::Having => KeywordPlace::AfterCondition(clause),
            Clause::GroupBy => KeywordPlace::AfterGroupItem,
            Clause::OrderBy => KeywordPlace::AfterOrderItem,
            Clause::Set => KeywordPlace::AfterSet,
            Clause::Values => KeywordPlace::AfterValues,
            _ => KeywordPlace::Other,
        })
    }

    /// A `*` (or `t.*`) of a select list right before the cursor.
    pub(super) fn star(&self, snap: &Snapshot, offset: usize) -> Option<Star> {
        if snap.state.clause != Clause::Select || snap.group.is_some() {
            return None;
        }
        let b = self.hole.checked_sub(1)?;
        let t = self.sig[b];
        if t.text(self.text) != "*" || t.end != offset {
            return None;
        }
        if b >= 2
            && self.kind(b - 1) == Some(TokenKind::Dot)
            && let Some(q) = self.name_at(b - 2)
        {
            return Some(Star { range: self.sig[b - 2].start..t.end, qualifier: Some(q) });
        }
        let before = b.checked_sub(1).map(|p| self.sig[p]);
        let starts = before.is_none_or(|p| {
            p.kind == TokenKind::Comma
                || ["SELECT", "DISTINCT", "ALL"].iter().any(|w| p.is_word(self.text, w))
        });
        starts.then_some(Star { range: t.range(), qualifier: None })
    }
}
