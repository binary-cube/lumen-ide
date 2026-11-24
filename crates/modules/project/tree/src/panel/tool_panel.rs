//! The panel as a side panel ([`ToolPanel`]): read, drawn, its header's buttons, the
//! mouse, the keys and the text, its commands and menus' targets, the drag let go.

use std::path::Path;
use std::sync::Arc;

use kernel::Point;
use ui_components::tooltip::Tip;
use ui_components::{
    KeyReach, PanelButton, PanelContext, PressMods, ToolPanel, ToolPanelAction, ViewerAction,
    ViewerCursor, ViewerKey, ViewerPaint,
};
use workbench_model::{ActionContext, EntriesRequest, ProjectView};
use workspace::Mark;

use crate::{TreeAction, operations};

use super::{Keys, ProjectPanel, TOGGLE, open};

impl ToolPanel for ProjectPanel {
    fn update(&mut self, ctx: &PanelContext<'_>) {
        self.active = ctx.path.map(Path::to_path_buf);
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(view) = data.downcast_ref::<ProjectView>() {
            self.follow(&view.events);
            self.search.indexing = view.indexing;
            self.set_project(view.queries.clone());
        } else if let Some(request) = data.downcast_ref::<EntriesRequest>() {
            self.request(request);
        }
    }

    fn poll(&mut self) -> bool {
        let found = self.finder.take().map(|results| self.search.set_results(results)).is_some();
        self.apply_changes() | found
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let (Some(waker), Ok(mut slot)) = (&ctx.waker, self.waker.lock())
            && slot.is_none()
        {
            *slot = Some(Arc::clone(waker));
        }
        self.restyle(ctx.palette);
        self.layout(ctx.bounds, ctx.scale);
        let Some(tree) = &mut self.tree else { return };
        // The panel grown (or rows gone) since the last scroll: kept within the new limit,
        // or the rows above stay hidden until the next scroll.
        self.view.scroll_by(0.0, 0.0, tree);
        self.search.scroll_by(0.0);
        let (palette, icons) = (ctx.palette, ctx.icons);
        let queries = self.queries.clone();
        let cache = &self.file_icons;
        cache.borrow_mut().stamp(icons);
        // The icons' registry decides (the chosen set's, else ours by language).
        let file_icon =
            |name: &str| cache.borrow_mut().icon(name, icons, palette, queries.as_deref());
        let folder_color = |name: &str| {
            let key = queries.as_ref()?.folder_color(name)?;
            Some(palette.folder_tint(key))
        };
        self.view.set_path_marks(queries.as_ref().and_then(|q| q.path_marks()));
        let search_focused = ctx.focused && self.keys == Keys::Search;
        self.search.paint_field(search_focused, icons, ctx.text, ctx.scene, palette);
        if self.search.is_active() {
            self.search.paint_results(icons, &file_icon, ctx.text, ctx.scene, palette);
        } else {
            self.view.paint(tree, ctx.text, ctx.scene, icons, &file_icon, &folder_color);
        }
        let mut ctx = ctx;
        self.paint_drag(&mut ctx, &file_icon);
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button =
            |id, icon, key: &str, group| PanelButton { id, icon, tooltip: i18n::t(key), group };
        Some(vec![
            button("new-file", "plus", "tooltip.new_file", 0),
            button("locate", "crosshair", "tooltip.locate", 0),
            button("expand", "chevrons-expand", "tooltip.expand", 1),
            button("collapse", "chevrons-collapse", "tooltip.collapse", 1),
            button("hide", "minus", "tooltip.hide", 2),
        ])
    }

    fn header_icon(&self) -> Option<&'static str> {
        Some("folder")
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            "new-file" => {
                self.new_entry(false);
            }
            "locate" => {
                if let Some(path) = self.active.clone() {
                    self.search.clear();
                    self.reveal(&path);
                }
            }
            "expand" => self.expand_more(),
            "collapse" => {
                if let Some(tree) = &mut self.tree {
                    tree.collapse_all();
                    self.view.scroll_by_pixels(f32::MIN, tree);
                }
            }
            "hide" => return Some(ToolPanelAction::View(ViewerAction::RunCommand(TOGGLE.into()))),
            _ => {}
        }
        None
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        if self.search.field_contains(p) {
            return self.press_search(p, clicks);
        }
        let action = self.click(p);
        // In the new entry's name: the cursor where it was clicked; a double click selects
        // it all.
        let input = match self.keys {
            Keys::NewEntry => self.view.pending_mut().map(|p| &mut p.input),
            Keys::Search | Keys::Tree => None,
        };
        if let Some(input) = input.filter(|i| i.contains(p)) {
            if clicks >= 2 { input.select_all() } else { input.click(p.x) }
        }
        action
    }

    /// ⌘ / Ctrl or ⇧ on a row: the selection grown, nothing opened.
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ToolPanelAction> {
        let on_rows = !self.search.is_active() && self.view.bounds().contains(p);
        if (mods.primary || mods.shift) && on_rows && self.view.pending().is_none() {
            let tree = self.tree.as_mut()?;
            if self.view.select_more(p, tree, mods.primary) {
                self.keys = Keys::Tree;
                return None;
            }
        }
        self.press(p, clicks)
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        // The tree's scrollbar (when shown): the rows scrolled there.
        if !self.search.is_active() && self.view.bar_press(p) {
            return None;
        }
        if self.search.field_contains(p) {
            return self.press_search(p, 1);
        }
        if self.search.list_contains(p) {
            if let Some(index) = self.search.result_at(p) {
                self.take_result(index);
            }
            return self.action.take();
        }
        let action = self.view.click(p, self.tree.as_mut()?)?;
        // A click elsewhere keeps the name typed (as ↵), or gives up an empty one; a rename
        // asked for goes first (the row clicked waits for the next click).
        self.settle_entry();
        if let Some(asked) = self.action.take() {
            return Some(asked);
        }
        self.keys = Keys::Tree;
        self.queries.as_ref()?;
        let tree = self.tree.as_mut()?;
        let (TreeAction::Toggle(id) | TreeAction::Open(id)) = action;
        self.press = Some((tree.path(id), p));
        match action {
            TreeAction::Toggle(dir) => {
                self.toggle_dir(dir);
                None
            }
            TreeAction::Open(file) => Some(open(tree.path(file), None)),
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.search.hover(p);
        if let Some(tree) = &mut self.tree {
            changed |= self.view.hover(p, tree);
        }
        if !self.search.is_active() {
            changed |= self.view.bar_hover(p);
        }
        changed
    }

    fn clickable(&self, p: Point) -> bool {
        let row = !self.search.is_active() && self.view.bounds().contains(p);
        row || self.search.on_clear(p) || self.search.list_contains(p)
    }

    fn focus_lost(&mut self) {
        if self.keys == Keys::NewEntry {
            self.settle_entry();
        }
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.search.cursor(p)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let tree = self.tree.as_ref()?;
        if self.search.is_active() {
            return None;
        }
        // A mark: what it means.
        if let Some((rect, id)) = self.view.mark_at(p) {
            let key = match tree.mark(id) {
                _ if tree.is_absent(id) => "absent",
                Mark::None => "absent",
                Mark::Local => "local",
                Mark::Changed => "changed",
                Mark::Behind => "behind",
                Mark::Conflict => "conflict",
            };
            return Some(Tip::new(rect, i18n::t(&format!("files.mark_{key}"))));
        }
        // A note: all of it.
        if let Some((rect, id)) = self.view.note_at(p)
            && let Some(note) = tree.note(id)
        {
            return Some(Tip::new(rect, note.text.to_string()));
        }
        // A name cut short: the whole of it.
        let (rect, id) = self.view.cut_name_at(p)?;
        Some(Tip::new(rect, tree.name(id)))
    }

    fn scroll(&mut self, dy: f32) -> bool {
        if self.search.is_active() {
            return self.search.scroll_by(dy);
        }
        let Some(tree) = &mut self.tree else { return false };
        self.view.scroll_by(0.0, dy, tree)
    }

    fn wants_text(&self) -> bool {
        matches!(self.keys, Keys::Search | Keys::NewEntry)
    }

    /// Its rows have the keys: its commands' own (Rename's ⇧F6) work here, before the
    /// editor's.
    fn keyed(&self) -> bool {
        self.keys == Keys::Tree
    }

    fn text_input(&mut self, text: &str) -> bool {
        match self.keys {
            Keys::Search => {
                if self.search.text(text).1 {
                    self.refresh_search();
                }
            }
            Keys::NewEntry => {
                let Some(pending) = self.view.pending_mut() else { return false };
                pending.input.insert(text);
                self.validate_entry();
            }
            Keys::Tree => return false,
        }
        true
    }

    /// Typing on the tree's rows searches: the search field takes the keyboard, the text
    /// typed replacing what it held (spaces alone don't start it).
    fn text_on_rows(&mut self, text: &str) -> bool {
        if self.keys != Keys::Tree || text.trim().is_empty() || text.chars().any(char::is_control) {
            return false;
        }
        self.view.cancel_new();
        self.keys = Keys::Search;
        self.search.field.set_text("");
        self.text_input(text)
    }

    fn selected_text(&self) -> Option<String> {
        let input = match self.keys {
            Keys::Search => return self.search.selected_text(),
            Keys::NewEntry => &self.view.pending()?.input,
            Keys::Tree => return None,
        };
        Some(input.selected_text().to_owned()).filter(|s| !s.is_empty())
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match self.keys {
            Keys::Search => self.search_key(key, reach),
            Keys::NewEntry => self.entry_key(key, reach),
            // Delete (Explorer's key), ⌫ (a Mac's "delete"; ⌘⌫ comes as a command): the
            // selection to the trash, asked first as from the menu.
            Keys::Tree if matches!(key, ViewerKey::Delete | ViewerKey::Backspace) => {
                self.command("delete")
            }
            Keys::Tree => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        let input = match self.keys {
            Keys::Search => {
                let (used, edited) = self.search.command(id);
                if edited {
                    self.refresh_search();
                }
                return used;
            }
            Keys::NewEntry => match self.view.pending_mut() {
                Some(pending) => &mut pending.input,
                None => return false,
            },
            // The tree's entries: the module's commands, on its selection.
            Keys::Tree => {
                let command = match id {
                    "copy" => operations::COPY,
                    "cut" => operations::CUT,
                    "paste" => operations::PASTE,
                    "delete" => operations::DELETE,
                    "rename" => operations::RENAME,
                    _ => return false,
                };
                if self.view.selected().is_none() {
                    return false;
                }
                self.action =
                    Some(ToolPanelAction::View(ViewerAction::RunCommand(command.0.into())));
                return true;
            }
        };
        match id {
            "select_all" => input.select_all(),
            _ => return false,
        }
        true
    }

    fn context_at(&mut self, p: Point) -> Option<ActionContext> {
        if self.search.is_active() {
            return None;
        }
        let tree = self.tree.as_mut()?;
        // On a row: that one (selected), or every row selected when it's one of them. In the
        // empty room under the rows: what's selected, else the project's root — the
        // menu shows there too.
        let id = match self.view.node_at(p, tree) {
            Some(id) if self.view.is_selected(id) && self.view.selection(tree).len() > 1 => {
                self.keys = Keys::Tree;
                return Self::context_of(tree, &self.view.selection(tree));
            }
            Some(id) => {
                self.view.select(Some(id));
                id
            }
            None if self.view.bounds().contains(p) => {
                // What's selected and shown (a selection hidden since — its folder collapsed —
                // isn't what's shown), else the root.
                tree.refresh_visible();
                let selected = self.view.selection(tree);
                self.keys = Keys::Tree;
                if selected.is_empty() {
                    return Self::context_of(tree, &[tree.root()]);
                }
                return Self::context_of(tree, &selected);
            }
            None => return None,
        };
        self.keys = Keys::Tree;
        Self::context_of(tree, &[id])
    }

    fn context(&self) -> Option<ActionContext> {
        if self.keys != Keys::Tree {
            return None;
        }
        let tree = self.tree.as_ref()?;
        Self::context_of(tree, &self.view.selection(tree))
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.selecting {
            return self.search.drag(p);
        }
        if self.view.bar_drag(p) {
            return true;
        }
        let scale = self.view.scale();
        self.drag_to(p, scale)
    }

    /// Its scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.view.next_frame()
    }

    fn release(&mut self) -> bool {
        if std::mem::take(&mut self.selecting) {
            return false;
        }
        if self.view.bar_release() {
            return true;
        }
        self.press = None;
        let Some(drag) = self.drag.take() else { return false };
        self.view.set_drop_target(None);
        if let Some(into) = drag.into {
            self.action = Some(ToolPanelAction::Move { from: drag.path, into });
        }
        true
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    fn keeps_keyboard(&mut self) -> bool {
        !std::mem::take(&mut self.hand_over)
    }
}
