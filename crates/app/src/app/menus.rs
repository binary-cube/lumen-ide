//! The context menus (tree, tabs, terminal tabs): drawn by us, the same on every system.

use super::*;

impl EditorApp {
    /// The context menu of the tab under the cursor (right click).
    /// Right click on the tree: selects the node and shows its menu.
    pub(super) fn tree_context_menu(&mut self, position: Point) -> bool {
        // What was right-clicked (the tree's row, selected), the menu's context from now on.
        let Some(context) = self.workbench.entries_menu_at(position) else { return false };
        let Some(target) = context.targets.first() else { return false };
        self.pointer.tree_menu_path = Some(target.path.clone());
        self.pointer.menu_position = Some(position);
        self.redraw();
        // Its rows and the modules' menus (Git ›…), asked with that context.
        let entries = self.workbench.context_menu(context.place);
        self.show_entries(entries, Vec::new(), position);
        true
    }

    /// Right click on a side panel with its own menu (the "Database" window's rows).
    pub(super) fn side_panel_menu(&mut self, position: Point) -> bool {
        let Some(entries) = self.workbench.side_menu_at(position) else { return false };
        self.show_entries(entries, Vec::new(), position);
        true
    }

    /// Right click on the projects list (the top bar's or the empty screen's): a project's
    /// or a group's menu.
    pub(super) fn projects_list_menu(&mut self, position: Point) -> bool {
        let Some(entries) = self.workbench.projects_menu_at(position) else { return false };
        self.show_entries(entries, Vec::new(), position);
        true
    }

    /// Right click on a module's tool strip button (Git): its menu (Pull, Push, Commit…);
    /// on the top bar's badge, the menu its module named.
    pub(super) fn tool_strip_menu(&mut self, position: Point) -> bool {
        let opened = self.workbench.top_badge_menu_at(position)
            || self.workbench.tool_strip_menu_at(position);
        if opened {
            self.redraw();
        }
        opened
    }

    /// Right click on a bottom window's tab: Rename / Pin / the closes / the splits.
    pub(super) fn tool_tab_context_menu(&mut self, position: Point) -> bool {
        let Some(tab) = self.workbench.tool_tab_menu_at(position) else {
            return false;
        };
        let makes_views = self.workbench.tool_makes_views();
        self.show_menu(cmd::tool_tab_menu(tab, makes_views), position);
        true
    }

    /// Right click in the bottom window's view: its own menu (a Git history's commit or
    /// file), if it has one there.
    pub(super) fn tool_view_context_menu(&mut self, position: Point) -> bool {
        let Some(entries) = self.workbench.tool_view_menu_at(position) else { return false };
        self.show_entries(entries, Vec::new(), position);
        true
    }

    /// Right click in the editor: the cursor goes there (unless it's inside the selection),
    /// then the editor's menu (Rename…, Go to Declaration, the clipboard).
    /// The active tab's view's own menu at `position` (a grid's), if it has one there.
    pub(super) fn viewer_context_menu(&mut self, position: Point) -> bool {
        if self.workbench.focus_group_at(position) {
            self.redraw();
        }
        let Some(entries) = self.workbench.viewer_menu_at(position) else { return false };
        self.show_entries(entries, Vec::new(), position);
        true
    }

    pub(super) fn editor_context_menu(&mut self, position: Point) -> bool {
        if self.workbench.focus_group_at(position) {
            self.redraw();
        }
        let placed = self.with_text(|wb, text| wb.editor_context_click(text, position));
        if placed != Some(true) {
            return false;
        }
        self.workbench.clear_menu_target();
        let entries = self.workbench.context_menu(workbench_model::MenuPlace::Editor);
        self.show_entries(entries, Vec::new(), position);
        true
    }

    /// Shows `items` in our context menu at `position` (its commands come back like the
    /// native menus').
    fn show_menu(&mut self, items: Vec<MenuItem>, position: Point) {
        self.show_menu_with(items, Vec::new(), position);
    }

    /// `items`, then the modules' submenus (`extra`) after a separator.
    fn show_menu_with(
        &mut self,
        items: Vec<MenuItem>,
        extra: Vec<ui_workbench::MenuEntry>,
        position: Point,
    ) {
        self.show_entries(cmd::menu_entries(items), extra, position);
    }

    /// `entries`, then the modules' submenus (`extra`) after a separator.
    pub(super) fn show_entries(
        &mut self,
        mut entries: Vec<ui_workbench::MenuEntry>,
        extra: Vec<ui_workbench::MenuEntry>,
        position: Point,
    ) {
        if !extra.is_empty() {
            entries.push(ui_workbench::MenuEntry::Separator);
            entries.extend(extra);
        }
        self.workbench.open_context_menu(entries, position);
        self.redraw();
    }

    pub(super) fn tab_context_menu(&mut self, position: Point) {
        // The tab may be in another editor group: that one becomes current.
        if self.workbench.focus_group_at(position) {
            self.redraw();
        }
        let (Some(window), Some(tab)) = (self.window.clone(), self.workbench.tab_at(position))
        else {
            return;
        };
        self.pointer.menu_position = Some(position);
        let path = self.workbench.tab_path(tab);
        let in_project = path.is_some()
            && self.workbench.tab_relative_path(tab).is_some_and(|p| p.is_relative());
        // Compare with › what the modules list (the other tabs with files), then their items
        // (the clipboard).
        let mut compare = Vec::new();
        if path.is_some() {
            compare = self.workbench.listed_items(
                workbench_model::MenuPlace::Tab,
                workbench_model::MenuSection::Compare,
                path.as_deref(),
            );
            let contributed = self.workbench.section_items(
                workbench_model::MenuPlace::Tab,
                workbench_model::MenuSection::Compare,
            );
            if !contributed.is_empty() {
                compare.push(MenuItem::Separator);
                compare.extend(contributed);
            }
        }
        let state = cmd::TabMenuState {
            count: self.workbench.tab_count(),
            has_path: path.is_some(),
            in_project,
            pinned: self.workbench.tab_pinned(tab),
            can_reopen: self.workbench.can_reopen(),
            locked: self
                .workbench
                .tab_lock(tab)
                .map(|l| l != crate::workbench::LockState::Writable),
            renamable: self.workbench.tab_renamable(tab),
        };
        let entries = cmd::tab_context_menu(
            (tab, self.workbench.tab_id(tab).get()),
            state,
            &cmd::file_manager_label(window.file_manager()),
            compare,
        );
        // The modules' menus (Git ›, Local History ›) act on the tab's file, if it's the
        // project's.
        self.workbench.set_menu_target(path.filter(|_| in_project));
        let extra = if in_project {
            self.workbench.contributed_menus(workbench_model::MenuPlace::TreeFile)
        } else {
            Vec::new()
        };
        self.show_entries(entries, extra, position);
    }
}
