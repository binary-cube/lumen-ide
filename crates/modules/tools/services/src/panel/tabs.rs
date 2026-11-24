//! The shown node's tabs: made again when it changes, their views made when first shown
//! and let go, shown, closed, pinned; what a view asks of the panel or the app; a tab's
//! menu.

use std::collections::HashSet;

use ui_components::{ServiceId, ServiceTab, ToolPanelAction, ViewMenuEntry, Viewer, ViewerAction};

use crate::tree::Key;

use super::{MenuTarget, ServicesPanel, TAB_LINK};

impl ServicesPanel {
    pub(super) fn refresh_tabs(&mut self) {
        let key = self.selected.filter(|k| k.node().is_some());
        let generation = key.and_then(|k| {
            self.contributors.iter().find(|c| c.id == k.provider()).map(|c| c.services.generation())
        });
        let of = key.zip(generation);
        if self.tabs_of == of && of.is_some() {
            return;
        }
        if of.is_none() && self.tabs_of.is_none() && self.tabs.is_empty() {
            return;
        }
        self.tabs_of = of;
        self.tabs = match key {
            Some(Key::Node(p, node)) => {
                self.contributor(p).map(|c| c.services.tabs(node)).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        // The views of tabs not shown any more let go.
        let shown: HashSet<ServiceId> = self.tabs.iter().map(|t| t.id).collect();
        self.views.retain(|id, _| shown.contains(id));
        if let Some(key) = key {
            let active = self.active_tab.get(&key).copied().filter(|id| shown.contains(id));
            // A node that is one of its tabs (a result under its session): that tab shown.
            let itself = key.node().filter(|id| shown.contains(id));
            match active.or(itself).or_else(|| self.tabs.first().map(|t| t.id)) {
                Some(id) => {
                    self.active_tab.insert(key, id);
                }
                None => {
                    self.active_tab.remove(&key);
                }
            }
        }
        if self.tabs.is_empty() {
            self.in_view = false;
        }
    }

    /// The shown node's active tab.
    pub fn active_tab(&self) -> Option<ServiceId> {
        self.selected.and_then(|k| self.active_tab.get(&k).copied())
    }

    /// The active tab's view, made if it isn't.
    pub(crate) fn active_view(&mut self) -> Option<&mut Box<dyn Viewer>> {
        let id = self.active_tab()?;
        if !self.views.contains_key(&id) {
            let provider = self.selected?.provider();
            let mut view = self.contributor(provider)?.services.open_tab(id)?;
            if let Some(sizes) = &self.sizes {
                view.set_data(sizes);
            }
            view.set_text_options(&self.text_options);
            self.views.insert(id, view);
        }
        self.views.get_mut(&id)
    }

    /// How many tabs' views are made (tests: let go when not shown).
    pub fn views_made(&self) -> usize {
        self.views.len()
    }

    pub fn tabs(&mut self) -> &[ServiceTab] {
        self.refresh();
        &self.tabs
    }

    /// Shows tab `i` of the node shown.
    pub fn show_tab(&mut self, i: usize) {
        let (Some(key), Some(tab)) = (self.selected, self.tabs.get(i)) else { return };
        self.active_tab.insert(key, tab.id);
    }

    /// Closes tab `i` (if it can be): its contributor told, its view let go.
    pub fn close_tab(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i).cloned().filter(|t| t.closable) else { return };
        let Some(provider) = self.selected.map(|k| k.provider()) else { return };
        if let Some(c) = self.contributor(provider) {
            c.services.close_tab(tab.id);
        }
        self.views.remove(&tab.id);
        self.tabs_of = None;
        self.refresh_tabs();
    }

    /// Pins / unpins tab `i`.
    pub fn toggle_pin(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i).cloned() else { return };
        let Some(provider) = self.selected.map(|k| k.provider()) else { return };
        if let Some(c) = self.contributor(provider) {
            c.services.pin_tab(tab.id, !tab.pinned);
        }
        self.tabs_of = None;
        self.refresh_tabs();
    }

    /// Closes the tabs but `i` (the pinned ones and those that can't close kept).
    pub fn close_other_tabs(&mut self, i: usize) {
        let Some(keep) = self.tabs.get(i).map(|t| t.id) else { return };
        let others: Vec<ServiceId> = self
            .tabs
            .iter()
            .filter(|t| t.id != keep && t.closable && !t.pinned)
            .map(|t| t.id)
            .collect();
        for id in others {
            if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                self.close_tab(i);
            }
        }
    }

    /// What the view asked for: a close closes its tab; the rest goes to the app.
    pub(super) fn view_action(&mut self, action: Option<ViewerAction>) -> Option<ToolPanelAction> {
        match action? {
            ViewerAction::Close => {
                let active = self.active_tab()?;
                let i = self.tabs.iter().position(|t| t.id == active)?;
                self.close_tab(i);
                None
            }
            // A link to another tab of the node shown (the Output's line naming a profile).
            ViewerAction::OpenLink(link) if link.starts_with(TAB_LINK) => {
                let id = u128::from_str_radix(&link[TAB_LINK.len()..], 16).ok()?;
                let i = self.tabs.iter().position(|t| t.id.0 == id)?;
                self.show_tab(i);
                None
            }
            other => Some(ToolPanelAction::View(other)),
        }
    }

    /// What the shown view answered a press, a key or a command with: a menu it asked for
    /// under a button of its own goes to the app, shown under it as this panel's menu (the
    /// row chosen comes back to the view, [`ToolPanel::menu_command`]), nothing else done;
    /// else its action ([`Self::view_action`]).
    pub(super) fn view_answer(&mut self, action: Option<ViewerAction>) -> Option<ToolPanelAction> {
        if let Some((anchor, entries)) = self.active_view().and_then(|v| v.take_menu()) {
            self.menu_target = Some(MenuTarget::View);
            return Some(ToolPanelAction::MenuUnder { anchor, entries });
        }
        self.view_action(action)
    }

    /// What a view's action does here (tests).
    #[cfg(test)]
    pub(crate) fn view_action_for_tests(
        &mut self,
        action: Option<ViewerAction>,
    ) -> Option<ToolPanelAction> {
        self.view_action(action)
    }

    /// The tab's menu rows.
    pub(super) fn tab_menu(&self, i: usize) -> Vec<ViewMenuEntry> {
        let Some(tab) = self.tabs.get(i) else { return Vec::new() };
        let t = i18n::t;
        let mut rows = vec![
            ViewMenuEntry::item(
                "tab.pin",
                if tab.pinned { t("services.unpin_tab") } else { t("services.pin_tab") },
            )
            .with_icon("pin"),
        ];
        if tab.closable {
            rows.push(ViewMenuEntry::Separator);
            rows.push(ViewMenuEntry::item("tab.close", t("services.close_tab")).with_icon("x"));
        }
        if self.tabs.len() > 1 {
            rows.push(ViewMenuEntry::item("tab.close_others", t("services.close_other_tabs")));
        }
        rows
    }
}
