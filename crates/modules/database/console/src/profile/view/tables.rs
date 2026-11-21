//! The profile's tables, and another run compared with it.

use super::*;

impl ProfileView {
    /// The profile's tables' statistics read in the background, if they aren't known and
    /// weren't asked for (once).
    pub(super) fn ask_stats(&mut self) {
        if self.stats_asked {
            return;
        }
        let known = self.facts.as_ref().is_some_and(|f| f.tables.iter().any(|t| t.stats.is_some()));
        let (Some(runner), Some(id)) = (self.runner.clone(), self.report().map(|r| r.profile.id))
        else {
            return;
        };
        self.stats_asked = true;
        if !known {
            runner.load_table_stats(id);
        }
    }

    /// The tables' view made again when its profile, its facts or the findings shown
    /// changed.
    pub(super) fn rebuild_tables(&mut self, report: &Arc<ProfileReport>, width: f32) {
        let facts_key = self.facts.as_ref().map_or(0, |f| Arc::as_ptr(f) as usize);
        let key = (report.profile.id, facts_key, self.shown_generation);
        if self.tables_made == Some(key) {
            let fit = crate::profile::tables::fitting(&self.tables_built.cols, width);
            if fit != self.tables_fit {
                let (columns, rows) = self.tables_built.narrowed(&fit);
                self.tables.set_columns(columns);
                let open: Vec<bool> =
                    (0..self.tables.rows().len()).map(|r| self.tables.is_open(r)).collect();
                self.tables.set_rows(rows, |i| open.get(i).copied().unwrap_or(true));
                self.tables_fit = fit;
            }
            return;
        }
        let first = self.tables_made.is_none_or(|m| m.0 != report.profile.id);
        let flavor = self
            .runner
            .as_ref()
            .and_then(Runner::dialect)
            .map_or(database::sql::Flavor::Postgres { version: None }, |d| d.flavor());
        let flavor = if report.profile.engine.driver == "sqlite" {
            database::sql::Flavor::Sqlite
        } else {
            flavor
        };
        let statement = self.set.as_ref().map(|s| s.statement.clone()).unwrap_or_default();
        let findings = self.shown_findings();
        let built = crate::profile::tables::build(
            report,
            &statement,
            flavor,
            &findings,
            self.facts.as_deref(),
        );
        let open: Vec<bool> =
            (0..self.tables.rows().len()).map(|r| self.tables.is_open(r)).collect();
        let fit = crate::profile::tables::fitting(&built.cols, width);
        let (columns, rows) = built.narrowed(&fit);
        self.tables.set_columns(columns);
        self.tables.set_empty(i18n::t("profile.tables.empty"));
        if first {
            self.tables.set_rows(rows, |_| true);
        } else {
            self.tables.set_rows(rows, |i| open.get(i).copied().unwrap_or(true));
        }
        self.tables_fit = fit;
        self.tables_built = built;
        self.tables_made = Some(key);
    }

    /// The table chosen in the tables' view: its first step chosen in the others (an index:
    /// the step that read through it, else its table's first).
    pub(super) fn tables_chosen(&mut self) {
        let Some(row) = self.tables.selected() else { return };
        let node = match self.tables_built.kinds.get(row) {
            Some(TablesRow::Table(u)) => {
                self.tables_built.usage.get(*u).and_then(|u| u.scans.first().map(|s| s.node))
            }
            Some(TablesRow::Index(u, name)) => self.tables_built.usage.get(*u).and_then(|u| {
                u.scans
                    .iter()
                    .find(|s| s.index.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(name)))
                    .or(u.scans.first())
                    .map(|s| s.node)
            }),
            None => None,
        };
        if node.is_some() && node != self.selected_node() {
            self.select_node(node);
        }
    }

    /// The table chosen in the tables' view opened (its data, or `structure`).
    pub(super) fn open_chosen_table(&mut self, structure: bool) {
        let Some(row) = self.tables.selected() else { return };
        let u = match self.tables_built.kinds.get(row) {
            Some(TablesRow::Table(u) | TablesRow::Index(u, _)) => *u,
            None => return,
        };
        let Some(table) = self.tables_built.usage.get(u).map(|u| u.table.clone()) else { return };
        let Some(runner) = self.runner.clone() else { return };
        let Some(access) =
            runner.project().and_then(crate::consoles::consoles).and_then(|c| c.access())
        else {
            return;
        };
        let opening = if structure {
            access.open_structure(runner.source(), &table)
        } else {
            access.open_table(runner.source(), &table)
        };
        self.action = opening.map(|o| match o {
            database::diagram::Opening::File(path, asked) => {
                ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                    host.open_file_with(&path, &asked);
                }))
            }
            database::diagram::Opening::View(v) => ViewerAction::OpenView(
                ui_components::OpenView::shared(v.title, v.extension, v.data),
            ),
        });
    }

    /// What the tables' view says under its rows: its statistics being read, or not to be
    /// had (`None`: they're there, or nothing to say).
    pub(super) fn tables_note(&self) -> Option<String> {
        if self.facts.as_ref().is_some_and(|f| f.tables.iter().any(|t| t.stats.is_some())) {
            return None;
        }
        if self.tables_built.usage.is_empty() {
            return None;
        }
        let busy = self.runner.as_ref().is_some_and(Runner::busy);
        Some(i18n::t(if self.stats_asked && busy {
            "profile.tables.reading"
        } else {
            "profile.tables.no_stats"
        }))
    }

    /// The tables' view's row of the table step `node` reads, chosen there too (one of its
    /// indexes chosen stays chosen).
    pub(super) fn sync_tables(&mut self, node: Option<NodeId>) {
        let Some(node) = node else { return };
        let kinds = &self.tables_built.kinds;
        let usage = &self.tables_built.usage;
        let Some(table) = usage.iter().position(|u| u.nodes().contains(&node)) else { return };
        let chosen = self.tables.selected().and_then(|r| kinds.get(r));
        let already = match chosen {
            Some(TablesRow::Table(u) | TablesRow::Index(u, _)) => *u == table,
            None => false,
        };
        if !already {
            let row = kinds.iter().position(|k| *k == TablesRow::Table(table));
            self.tables.select(row);
        }
    }

    /// The profile shown compared with the console's profile `other` (the older one
    /// before): `false` when there's none of that id, or it's this one.
    pub fn compare_with(&mut self, other: database::profile::ProfileId) -> bool {
        let (Some(runner), Some(own)) = (self.runner.clone(), self.report().cloned()) else {
            return false;
        };
        if own.profile.id == other {
            return false;
        }
        let Some(theirs) = runner.profile_report(other) else { return false };
        let list = runner.profile_list();
        let pos = |id| list.iter().position(|b: &crate::profile::store::ProfileBrief| b.id == id);
        let label = |id, r: &ProfileReport| match list.iter().find(|b| b.id == id) {
            Some(b) => crate::profile::compare::side_label(&b.at, b.time_ms),
            None => crate::profile::compare::side_label(
                "",
                r.totals.time_ms.or(r.profile.statement.wall_ms),
            ),
        };
        // The newer one is after (one let go is older than those kept).
        let own_after = match (pos(own.profile.id), pos(other)) {
            (Some(a), Some(b)) => a > b,
            (None, _) => false,
            (Some(_), None) => true,
        };
        let mine = Side { label: label(own.profile.id, &own), report: Arc::clone(&own) };
        let other = Side { label: label(other, &theirs), report: theirs };
        self.compare = Some(if own_after {
            CompareView::new(other, mine, true)
        } else {
            CompareView::new(mine, other, false)
        });
        true
    }

    /// The comparison left.
    pub fn close_compare(&mut self) -> bool {
        self.compare.take().is_some()
    }

    /// Compare ▾'s rows: the console's other profiles, the same query's first (recommended),
    /// the newest first; "Stop Comparing" while comparing.
    pub(super) fn compare_items(&self) -> Vec<(String, String, &'static str, bool)> {
        let mut items = Vec::new();
        if self.compare.is_some() {
            items.push(("compare.close".to_owned(), i18n::t("profile.compare.close"), "x", true));
        }
        let (Some(runner), Some(own)) = (self.runner.as_ref(), self.report()) else {
            return items;
        };
        let list = runner.profile_list();
        let mine = list.iter().find(|b| b.id == own.profile.id).map(|b| b.fingerprint);
        let mut others: Vec<&crate::profile::store::ProfileBrief> =
            list.iter().rev().filter(|b| b.id != own.profile.id).collect();
        others.sort_by_key(|b| mine.is_none_or(|m| b.fingerprint != m));
        for b in others.iter().take(20) {
            let same = mine.is_some_and(|m| m == b.fingerprint);
            let label = crate::profile::compare::side_label(&b.at, b.time_ms);
            let label = if same {
                i18n::tr!("profile.compare.item_same", label = label.as_str())
            } else {
                let what: String = if b.statement.chars().count() > 40 {
                    b.statement.chars().take(38).chain(std::iter::once('…')).collect()
                } else {
                    b.statement.clone()
                };
                i18n::tr!("profile.compare.item", label = label.as_str(), statement = what.as_str())
            };
            let icon = if same { "compare" } else { "plan" };
            items.push((format!("profile.compare:{}", b.id), label, icon, true));
        }
        if others.is_empty() {
            items.push((String::new(), i18n::t("profile.compare.no_other"), "info", false));
        }
        items
    }
}
