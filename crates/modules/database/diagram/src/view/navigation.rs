//! Moving around the diagram: the places gone to and back, a table chosen and its
//! neighbors, a hit acted on, a card's menu.

use super::*;

impl RelationsView {
    pub(super) fn place(&self) -> Place {
        Place { focus: self.focus, depth: self.depth, selected: self.selected }
    }

    /// Goes to `place` (the current one remembered for ←).
    pub(super) fn go(&mut self, place: Place) {
        if place == self.place() {
            self.center_selected();
            return;
        }
        let before = self.place();
        self.history.push(before.clone());
        let regroup = place.focus != before.focus || place.depth != before.depth;
        self.apply(place, regroup);
    }

    fn apply(&mut self, place: Place, regroup: bool) {
        self.focus = place.focus;
        self.depth = place.depth;
        self.selected = place.selected;
        if regroup {
            self.refit = true;
            self.relayout();
        } else {
            self.center_selected();
        }
    }

    pub fn back(&mut self) -> bool {
        let Some(place) = self.history.pop() else { return false };
        let regroup = place.focus != self.focus || place.depth != self.depth;
        self.apply(place, regroup);
        true
    }

    /// Table `t` selected and centered.
    pub fn select(&mut self, t: usize) {
        self.go(Place { selected: Some(t), ..self.place() });
    }

    /// Table `t`'s neighbors shown (`depth` links away).
    pub fn show_neighbors(&mut self, t: usize, depth: usize) {
        self.go(Place { focus: Some(t), depth, selected: Some(t) });
    }

    /// Every table shown again.
    pub fn show_all(&mut self) {
        self.go(Place { focus: None, depth: self.depth, selected: self.selected });
    }

    /// The filter's text changed: what matches is shown.
    pub(super) fn filtered(&mut self) {
        self.refit = true;
        self.relayout();
    }

    /// What opens table `t`'s data.
    pub(super) fn open(&self, t: usize) -> Option<ViewerAction> {
        let request = self.request.as_ref()?;
        let table = self.graph.as_ref()?.tables.get(t).filter(|t| !t.external)?.table().clone();
        Some(match request.access.open(&table)? {
            Opening::File(path, asked) => {
                ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                    host.open_file_with(&path, &asked);
                }))
            }
            Opening::View(v) => ViewerAction::OpenView(ui_components::OpenView::shared(
                v.title,
                v.extension,
                v.data,
            )),
        })
    }

    pub(super) fn hit(&self, p: Point) -> Option<Hit> {
        match self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h) {
            Some(hit) => Some(hit),
            None => self.edge_at(p).map(Hit::Edge),
        }
    }

    /// What a click on `hit` (at screen point `at`) does.
    pub(super) fn activate(&mut self, hit: Hit, at: Point) {
        match hit {
            Hit::Back => {
                self.back();
            }
            Hit::Card(t) | Hit::Row(t, _) => {
                self.clicked = Some(t);
                self.select(t);
            }
            Hit::Depth(d) => {
                if let Some(f) = self.focus
                    && d != self.depth
                {
                    self.show_neighbors(f, d);
                }
            }
            Hit::ShowAll => self.show_all(),
            Hit::Kinds(kinds) => self.show_kinds(kinds),
            Hit::Found => self.toggle_list(),
            Hit::Accept(i) => {
                self.accept(i, true);
            }
            Hit::Ignore(i) => {
                self.accept(i, false);
            }
            Hit::Unaccept(i) => {
                self.undo_decision(i, true);
            }
            Hit::Unignore(i) => {
                self.undo_decision(i, false);
            }
            Hit::ListRow(t) => self.select(t),
            Hit::Edge(link) => {
                let at = self.view.to_diagram(at);
                self.open_edge(link, Some(at));
            }
            Hit::PopAccept => {
                self.decide_edge(Some(true));
            }
            Hit::PopIgnore => {
                self.decide_edge(Some(false));
            }
            Hit::PopUndo => {
                self.decide_edge(None);
            }
            Hit::GoTo(t) => {
                self.popover = None;
                self.select(t);
            }
            Hit::Field | Hit::List | Hit::Bar | Hit::Popover => {}
        }
    }

    /// The menu of table `t`'s card.
    pub(super) fn card_menu(&self, t: usize) -> Vec<ViewMenuEntry> {
        let item = |id: String, key: &str| ViewMenuEntry::item(id, i18n::t(key));
        let external = self.graph.as_ref().and_then(|g| g.tables.get(t)).is_none_or(|t| t.external);
        let mut entries = Vec::new();
        if !external {
            entries.push(item(format!("open:{t}"), "relations.menu_open").with_icon("table"));
        }
        entries.push(
            item(format!("neighbors:{t}"), "relations.menu_neighbors").with_icon("relations"),
        );
        if self.focus.is_some() {
            entries.push(item("all".into(), "relations.menu_all"));
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(item(format!("copy:{t}"), "relations.menu_copy").with_icon("copy"));
        entries
    }
}
