//! What the list shows: the sessions narrowed by the search, the favorites and the agent,
//! arranged into folders; the agents' filter and its list.

use i18n::t;
use ui_components::picker::{PickerEvent, PickerItem};

use super::{Row, SEARCHED, SessionsPanel};

impl SessionsPanel {
    /// Something is searched for (the folders holding what's found shown open).
    pub(super) fn searched(&self) -> bool {
        !self.query.text().trim().is_empty()
    }

    /// The sessions shown made again when the read or the search changed: those whose title
    /// has every word searched (any case), in the read's order.
    pub(super) fn filter(&mut self) {
        // Compared first without taking anything (every paint): the list only on a change.
        let at = self.project.map_or(0, crate::favorites::generation);
        let query = self.query.text();
        let agent = self.agent_filter;
        if self.shown_for.as_ref().is_some_and(|(g, f, q, a)| {
            *g == self.generation && *f == at && q == query && *a == agent
        }) {
            return;
        }
        let key = (self.generation, at, query.to_owned(), agent);
        let (_, favorites, missing) = self.project.map(crate::favorites::list).unwrap_or_default();
        self.favorite = self
            .sessions
            .iter()
            .map(|s| favorites.iter().any(|f| f.agent == s.agent && f.id == s.id))
            .collect();
        let marks = self.project.map(crate::favorites::marks).unwrap_or_default();
        self.marked = self
            .sessions
            .iter()
            .map(|s| marks.iter().find(|m| m.agent == s.agent && m.id == s.id).cloned())
            .collect();
        let words: Vec<String> = key.2.split_whitespace().map(str::to_lowercase).collect();
        let matches = |title: &str| {
            let title = title.to_lowercase();
            words.iter().all(|w| title.contains(w.as_str()))
        };
        self.missing = favorites
            .into_iter()
            .filter(|f| missing.contains(&(f.agent.clone(), f.id.clone())) && matches(&f.title))
            .filter(|f| agent.is_none_or(|a| f.agent == a))
            .collect();
        self.shown = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                matches(&s.title)
                    && (!self.only_favorites || self.favorite[*i])
                    && agent.is_none_or(|a| s.agent == a)
            })
            .map(|(i, _)| i)
            .collect();
        // The pinned first, then the favorites (in the read's order), then the rest; in each,
        // those done last.
        let (favorite, marked) = (&self.favorite, &self.marked);
        self.shown.sort_by_key(|&i| {
            let mark = marked.get(i).and_then(Option::as_ref);
            let (pinned, done) = mark.map_or((false, false), |m| (m.pinned, m.done));
            (!pinned, !favorite.get(i).copied().unwrap_or(false), done)
        });
        self.shown_for = Some(key);
        self.arrange();
    }

    /// The rows from the sessions shown: the missing favorites, each folder with its
    /// sessions under it (open, or searched; a folder holding none of what's searched or
    /// filtered left out), then the sessions in none.
    fn arrange(&mut self) {
        self.folders = self.project.map(crate::folders::list).unwrap_or_default();
        let narrowed = self.searched() || self.only_favorites || self.agent_filter.is_some();
        let folder_of = |i: usize| -> Option<&str> {
            let id = self.marked.get(i)?.as_ref()?.folder.as_deref()?;
            self.folders.iter().any(|f| f.id == id).then_some(id)
        };
        let mut rows: Vec<Row> = (0..self.missing.len()).map(Row::Missing).collect();
        let mut counts = Vec::with_capacity(self.folders.len());
        for (f, folder) in self.folders.iter().enumerate() {
            let held: Vec<usize> =
                self.shown.iter().copied().filter(|&i| folder_of(i) == Some(&folder.id)).collect();
            counts.push(held.len());
            if narrowed && held.is_empty() {
                continue;
            }
            rows.push(Row::Folder(f));
            if folder.open || self.searched() {
                rows.extend(held.into_iter().map(|i| Row::Session { i, nested: true }));
            }
        }
        let loose: Vec<Row> = self
            .shown
            .iter()
            .copied()
            .filter(|&i| folder_of(i).is_none())
            .map(|i| Row::Session { i, nested: false })
            .collect();
        rows.extend(loose);
        self.rows = rows;
        self.folder_counts = counts;
    }

    /// Searching: all of the project's sessions looked through (read again for as many).
    pub(super) fn searching(&mut self) {
        self.scroll = 0.0;
        self.hovered = None;
        if !self.query.text().trim().is_empty() && self.limit < SEARCHED {
            self.limit = SEARCHED;
            self.read();
        }
    }

    /// The agents whose sessions were read (in the agents' order), for the filter.
    fn agents_read(&self) -> Vec<&'static str> {
        let mut read: Vec<&'static str> = self.icons.iter().map(|(id, _)| *id).collect();
        read.retain(|id| self.sessions.iter().any(|s| s.agent == *id));
        read
    }

    /// The filter is offered while the list mixes agents (or one is chosen: to undo it).
    pub(super) fn filters_agents(&self) -> bool {
        self.agent_filter.is_some() || self.agents_read().len() > 1
    }

    /// The agents' filter's list opened under it: every agent, then each one with sessions,
    /// the chosen one checked.
    pub(super) fn open_agent_filter(&mut self) {
        let mut ids = self.agents_read();
        if let Some(chosen) = self.agent_filter.filter(|a| !ids.contains(a)) {
            ids.push(chosen);
        }
        let mut rows: Vec<Option<&'static str>> = vec![None];
        rows.extend(ids.into_iter().map(Some));
        let items = rows
            .iter()
            .map(|row| match row {
                None => PickerItem { label: t("agents.all_agents"), ..PickerItem::default() },
                Some(id) => PickerItem {
                    label: self.agent_name(id),
                    icon: Some(
                        self.icons.iter().find(|(a, _)| a == id).map_or(crate::ICON, |(_, i)| *i),
                    ),
                    ..PickerItem::default()
                },
            })
            .collect();
        let chosen = rows.iter().position(|r| *r == self.agent_filter);
        self.filter_list.open(items, chosen);
        self.filter_rows = rows;
    }

    /// What the agents' list answered: a row chosen filters by its agent.
    pub(super) fn filter_list_event(&mut self, event: Option<PickerEvent>) -> bool {
        match event {
            Some(PickerEvent::Chosen(i)) => {
                match self.filter_rows.get(i).copied() {
                    Some(Some(id)) => self.filter_by_agent(id),
                    Some(None) => self.filter_by_agent(""),
                    None => {}
                }
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Agent `id`'s name (its id when it's no longer offered).
    pub(super) fn agent_name(&self, id: &str) -> String {
        self.agents().iter().find(|a| a.id() == id).map_or_else(|| id.to_owned(), |a| a.name())
    }

    /// Only agent `id`'s sessions listed (empty: every agent's).
    fn filter_by_agent(&mut self, id: &str) {
        let chosen = self.agents().iter().map(|a| a.id()).find(|a| *a == id);
        if chosen == self.agent_filter {
            return;
        }
        self.agent_filter = chosen;
        self.scroll = 0.0;
        self.hovered = None;
    }
}
