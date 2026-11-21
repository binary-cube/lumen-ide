//! The Remote tool window's "To upload" tab: a changes panel of what the server hasn't as
//! it is here — files that couldn't go (why), changed or made here, moved or deleted here
//! while uploads wait for the user, changed on the server too (a conflict) — as the shared
//! changed-files tree (tree or list, search, several chosen: ⌘ / ⇧-click, ⌘A). Its header:
//! the switch "Upload on save", "Upload N files" (those chosen), "Upload all". A right click:
//! Upload, Compare with the Server, Discard the Changes (asked first), Never Upload (the
//! file / folder, or `*.ext`). A double click or ↵ compares.
//!
//! A button's spinner turns while what it sent is going (checked, uploaded); pressed again
//! meanwhile, it does nothing. Uploading several checks the server for all of them first (see
//! [`crate::session::State::send`]); deletions are asked about first. The list is the
//! session's (see [`crate::outgoing`]: read in the background, the same the button counts);
//! opening the tab reads it again (a change made outside the editor shows).

use std::path::Path;
use std::sync::Arc;

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::changed_files::{
    ChangeState, ChangedFile, FilesTree, RowKind, TreeEvent, TreePart,
};
use ui_components::control::{Control, Controls, Press};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    PressMods, ViewMenuEntry, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::draw_text;

use crate::outgoing::{Rows, Why};

/// The extension of the view (in the Remote tool window).
pub const EXTENSION: &str = "view-remote-pending";

/// What the view is opened with: the remote project (its session), its queries (the files'
/// icons by language).
#[derive(Clone)]
pub struct PendingData {
    pub project: kernel::ProjectId,
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
}

/// Its right-click menu's rows (acting on the files chosen).
const UPLOAD: &str = "remote.pending.upload";
const COMPARE: &str = "remote.pending.compare";
const DISCARD: &str = "remote.pending.discard";
const EXCLUDE: &str = "remote.pending.exclude";
const EXCLUDE_EXT: &str = "remote.pending.exclude_ext";
const PROPERTIES: &str = "remote.pending.properties";

#[derive(Debug)]
pub(crate) struct PendingProvider;

impl ViewerProvider for PendingProvider {
    fn id(&self) -> &'static str {
        "remote.pending"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PendingList::default()))
    }
}

/// The header's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Switch,
    Chosen,
    All,
}

/// The place of the files tree among the view's parts (see [`PendingList::with_controls`]).
const TREE: usize = 1;

/// A button's rows still going (its spinner turns; pressed, it does nothing), from what
/// "Upload all" and "Upload N files" had going at the last paint.
fn is_busy(busy: (bool, bool), button: Button) -> bool {
    match button {
        Button::Switch => false,
        Button::All => busy.0,
        Button::Chosen => busy.1,
    }
}

/// The header's buttons as one control: lit under the mouse, a hand over those not busy;
/// the one pressed (not busy) is written to `pressed`.
struct HeaderButtons<'a> {
    buttons: &'a [(Rect, Button)],
    busy: (bool, bool),
    hover: &'a mut Option<Button>,
    pressed: &'a mut Option<Button>,
}

impl HeaderButtons<'_> {
    fn at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|&(_, b)| b)
    }
}

impl Control for HeaderButtons<'_> {
    fn hit(&self, p: Point) -> bool {
        self.at(p).is_some()
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let Some(button) = self.at(p) else { return Press::Ignored };
        if !is_busy(self.busy, button) {
            *self.pressed = Some(button);
        }
        Press::Took
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let under = p.and_then(|p| self.at(p));
        std::mem::replace(self.hover, under) != under
    }

    fn cursor(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        let button = self.at(p)?;
        (!is_busy(self.busy, button)).then_some(ui_components::ViewerCursor::Pointer)
    }
}

pub struct PendingList {
    project: Option<kernel::ProjectId>,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    /// The session's list (the tree's files, in its order).
    rows: Rows,
    /// The list's generation shown (0: not read yet), and that of what's being sent.
    generation: u64,
    sending: u64,
    tree: FilesTree,
    /// The header's buttons (from the last paint), and the one under the mouse.
    buttons: Vec<(Rect, Button)>,
    /// Its parts' state of use (see [`PendingList::with_controls`]).
    controls: Controls,
    /// "Upload all" and "Upload N files" still going, as of the last paint (what the mouse
    /// and a press look at: nothing read per move).
    busy: (bool, bool),
    hover: Option<Button>,
    /// What the right-clicked menu acts on.
    menu: Option<MenuTarget>,
    /// For the app (a command run on the rows chosen).
    action: Option<ViewerAction>,
}

/// What a right-click menu acts on: the rows chosen, and the folder right-clicked.
#[derive(Debug)]
struct MenuTarget {
    rows: Vec<(String, Why)>,
    folder: Option<String>,
}

impl Default for PendingList {
    fn default() -> Self {
        let mut tree = FilesTree::default();
        tree.count_key = "remote.pending_count";
        Self {
            project: None,
            queries: None,
            rows: Rows::default(),
            generation: 0,
            sending: 0,
            tree,
            buttons: Vec::new(),
            controls: Controls::default(),
            busy: (false, false),
            hover: None,
            menu: None,
            action: None,
        }
    }
}

impl std::fmt::Debug for PendingList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingList").field("rows", &self.rows.len()).finish_non_exhaustive()
    }
}

/// A row of the list as the tree shows it.
pub(crate) fn changed_file((path, why): &(String, Why)) -> ChangedFile {
    let state = match why {
        Why::Waiting(_) => ChangeState::Failed,
        Why::Changed => ChangeState::Modified,
        Why::New => ChangeState::Added,
        Why::Renamed { .. } => ChangeState::Renamed,
        Why::Deleted => ChangeState::Deleted,
        Why::Conflict => ChangeState::Conflicted,
    };
    let mut file = ChangedFile::new(path.clone(), state);
    match why {
        Why::Waiting(reason) => file.note = Some(reason.clone()),
        Why::Conflict => file.note = Some(t("remote.pending_conflict")),
        Why::Renamed { from } => file.old_path = Some(from.clone()),
        _ => {}
    }
    file
}

/// The extension all of `paths` share (`.log`), if they do.
fn shared_extension(paths: &[&str]) -> Option<String> {
    let ext = |p: &str| {
        let name = p.rsplit('/').next().unwrap_or(p);
        name.rfind('.').filter(|&i| i > 0).map(|i| name[i..].to_lowercase())
    };
    let first = ext(paths.first()?)?;
    paths.iter().all(|p| ext(p).as_deref() == Some(first.as_str())).then_some(first)
}

impl PendingList {
    fn state(&self) -> Option<Arc<crate::session::State>> {
        self.project.and_then(crate::session::session)
    }

    /// The rows chosen in the tree.
    fn chosen(&self) -> Vec<(String, Why)> {
        self.tree.chosen().into_iter().filter_map(|i| self.rows.get(i).cloned()).collect()
    }

    /// Its parts, the topmost first, with their state of use: the header's buttons, the
    /// files tree (it has the keys whenever nothing else took them, as the view's keyboard
    /// is its). What a press or a key asked for is done after.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let (mut pressed, mut event) = (None, None);
        let answer = {
            let mut header = HeaderButtons {
                buttons: &self.buttons,
                busy: self.busy,
                hover: &mut self.hover,
                pressed: &mut pressed,
            };
            let mut tree = TreePart { tree: &mut self.tree, event: &mut event };
            let mut parts: [&mut dyn Control; 2] = [&mut header, &mut tree];
            if self.controls.focused().is_none() {
                self.controls.focus(&mut parts, Some(TREE));
            }
            f(&mut self.controls, &mut parts)
        };
        match pressed {
            Some(Button::Switch) => self.flip_auto_upload(),
            Some(Button::Chosen) => self.upload(&self.chosen()),
            Some(Button::All) => self.upload(&self.rows),
            None => {}
        }
        if let Some(TreeEvent::Open(i)) = event
            && let Some(row) = self.rows.get(i).cloned()
        {
            self.compare(&row);
        }
        answer
    }

    /// What's going, looked at once (a paint): "Upload all"'s, and the chosen rows'.
    fn look_busy(&mut self, state: &crate::session::State) {
        let all = state.sending_any();
        let chosen = all && {
            let paths = self.tree.chosen().into_iter().filter_map(|i| self.rows.get(i));
            state.sending_among(paths.map(|(p, _)| p.as_str()))
        };
        self.busy = (all, chosen);
    }

    /// `rows` sent to the server (see [`crate::session::State::send`]).
    fn upload(&self, rows: &[(String, Why)]) {
        if let Some(state) = self.state() {
            state.send(rows);
        }
    }

    /// Row `row` compared with the server's copy (a move: with its name there).
    fn compare(&self, row: &(String, Why)) {
        let Some(state) = self.state() else { return };
        let there = match &row.1 {
            Why::Renamed { from } => from.as_str(),
            _ => row.0.as_str(),
        };
        state.compare_with_server(&row.0, there);
    }

    /// Saves upload at once, or wait for the user (the switch flipped).
    fn flip_auto_upload(&self) {
        let Some(state) = self.state() else { return };
        if let Err(why) = state.set_auto_upload(!state.auto_upload()) {
            state.failed_with("remote.auto_upload_failed", &why);
        }
    }

    /// The menu for `rows` (a folder's: `folder`).
    fn menu_for(rows: &[(String, Why)], folder: Option<&str>) -> Vec<ViewMenuEntry> {
        let mut menu =
            vec![ViewMenuEntry::item(UPLOAD, t("remote.menu_upload")).with_icon("upload")];
        if rows.len() == 1 {
            menu.push(ViewMenuEntry::item(COMPARE, t("remote.menu_compare")).with_icon("compare"));
        }
        menu.push(ViewMenuEntry::item(DISCARD, t("remote.menu_discard")).with_icon("undo"));
        menu.push(ViewMenuEntry::Separator);
        let name = match (folder, rows) {
            (Some(folder), _) => Some(format!("{folder}/")),
            (None, [(path, _)]) => Some(path.rsplit('/').next().unwrap_or(path).to_owned()),
            _ => None,
        };
        let label = match name {
            Some(name) => tr!("remote.menu_exclude", name = name.as_str()),
            None => t("remote.menu_exclude_these"),
        };
        menu.push(ViewMenuEntry::item(EXCLUDE, label).with_icon("eye-off"));
        let paths: Vec<&str> = rows.iter().map(|(p, _)| p.as_str()).collect();
        if let Some(ext) = shared_extension(&paths) {
            menu.push(
                ViewMenuEntry::item(
                    EXCLUDE_EXT,
                    tr!("remote.menu_exclude_ext", ext = ext.as_str()),
                )
                .with_icon("eye-off"),
            );
        }
        menu.push(ViewMenuEntry::Separator);
        menu.push(ViewMenuEntry::item(PROPERTIES, t("remote.menu_properties")).with_icon("info"));
        menu
    }
}

impl Viewer for PendingList {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<PendingData>() else { return };
        self.project = Some(data.project);
        self.queries.clone_from(&data.queries);
        if let Some(state) = crate::session::session(data.project) {
            crate::outgoing::refresh(&state);
        }
        self.poll();
    }

    /// The session's list, when it was read again.
    fn poll(&mut self) -> bool {
        let Some(state) = self.state() else { return false };
        let (generation, rows) = state.outgoing.rows();
        let changed = generation != self.generation;
        if changed {
            self.tree.set_files(rows.iter().map(changed_file).collect());
            (self.generation, self.rows) = (generation, rows);
        }
        let sending = state.sending_generation();
        changed | (std::mem::replace(&mut self.sending, sending) != sending)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.tree.scroll_by(dy)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press_with(p, 1, PressMods::default())
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.press_with(p, clicks, PressMods::default())
    }

    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press_with(parts, p, clicks, mods));
        None
    }

    fn wants_text(&self) -> bool {
        self.tree.searching
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        let line = text.lines().next().unwrap_or_default();
        self.with_controls(|c, parts| c.paste(parts, line)).is_some()
    }

    fn selected_text(&self) -> Option<String> {
        self.tree.selected_text()
    }

    fn select_all(&mut self) -> bool {
        !self.tree.searching && self.tree.choose_all()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, ui_components::KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        // Tab: the tree's own answer (kept while searching, else not used), not a move
        // of the focus among parts (it's the only one with keys: Tab would go nowhere).
        if key == ViewerKey::Tab {
            return self.tree.searching;
        }
        self.with_controls(|c, parts| c.key(parts, key, reach)).is_some()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let kind = self.tree.choose_at(p)?;
        let rows = self.chosen();
        if rows.is_empty() {
            return None;
        }
        let folder = match kind {
            RowKind::Folder { key, .. } => Some(key),
            RowKind::File { .. } => None,
        };
        let menu = Self::menu_for(&rows, folder.as_deref());
        self.menu = Some(MenuTarget { rows, folder });
        Some(menu)
    }

    fn command(&mut self, id: &str) -> bool {
        // The app's edit commands, for the search.
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        let Some(state) = self.state() else { return false };
        let Some(MenuTarget { rows, folder }) = self.menu.take() else { return false };
        match id {
            UPLOAD => state.send(&rows),
            COMPARE => {
                if let [row] = rows.as_slice() {
                    self.compare(row);
                }
            }
            DISCARD => state.discard(rows),
            EXCLUDE => match folder {
                Some(folder) => state.exclude(vec![folder]),
                None => state.exclude(rows.into_iter().map(|(p, _)| p).collect()),
            },
            PROPERTIES => {
                let paths = match folder {
                    Some(folder) => vec![folder],
                    None => rows.into_iter().map(|(p, _)| p).collect(),
                };
                self.action = Some(crate::properties_view::action(state.project, paths));
            }
            EXCLUDE_EXT => {
                let paths: Vec<&str> = rows.iter().map(|(p, _)| p.as_str()).collect();
                if let Some(ext) = shared_extension(&paths) {
                    state.exclude(vec![format!("*{ext}")]);
                }
            }
            _ => return false,
        }
        true
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    /// A fixed tab of the Remote window (as Git's Log and Changes): never closed.
    fn closable(&self) -> bool {
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    /// The files' scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.controls.tooltip_at(p)
    }

    /// A hand over the header's buttons (not over one still going).
    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.controls.cursor_at(p)
    }

    /// What was sent is going: the buttons' spinners turn.
    fn is_loading(&self) -> bool {
        self.state().is_some_and(|s| s.sending_any())
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.paint_list(ctx);
        // What the parts want of the frames, as they were just drawn.
        self.with_controls(|c, parts| c.settle(parts));
    }
}

impl PendingList {
    fn paint_list(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        self.buttons.clear();
        let Some(state) = self.state() else {
            ds::empty_state(layer, ctx.text, &ds, b, &t("remote.not_remote"), None);
            return;
        };
        // The header: the title; on the right "Upload all", "Upload N files", the switch.
        let header_h = px(44.0);
        let strong = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = ctx.text.shape_line(&t("remote.pending_tab"), &strong);
        let baseline = ds::optical_baseline(b.min_y(), header_h, &strong);
        draw_text(layer, &shaped, b.min_x() + px(16.0), baseline, p.text);
        let h = px(ds::TOOLBAR_BUTTON_H);
        let top = b.min_y() + ((header_h - h) / 2.0).round();
        let mut right = b.max_x() - px(16.0);
        let chosen = self.tree.chosen().len();
        self.look_busy(&state);
        let (all_busy, chosen_busy) = self.busy;
        let mut button = |layer: &mut render_api::Layer, control, label: String, icon, accent| {
            let width = ds::toolbar_button_width(ctx.text, &ds, &label);
            let at = Rect::new(right - width, top, width, h);
            let hovered = self.hover == Some(control);
            let spec = ds::ToolbarButton { label: &label, icon, accent, hovered };
            let busy = match control {
                Button::All => all_busy,
                Button::Chosen => chosen_busy,
                Button::Switch => false,
            };
            if busy {
                ds::busy_toolbar_button(layer, ctx.text, ctx.icons, &ds, at, spec);
            } else {
                ds::toolbar_button(layer, ctx.text, ctx.icons, &ds, at, spec);
            }
            self.buttons.push((at, control));
            right = at.min_x() - px(8.0);
        };
        if !self.rows.is_empty() {
            // In the accent: the main action.
            button(layer, Button::All, t("remote.upload_all"), "upload", true);
            if chosen > 0 && chosen < self.rows.len() {
                let label = tr!("remote.upload_selected", count = chosen);
                button(layer, Button::Chosen, label, "upload", false);
            }
            right -= px(8.0);
        }
        // "Upload on save": the switch, its label before it.
        let small = ds.ui_font(-1.0);
        let label = ctx.text.shape_line(&t("remote.auto_upload_switch"), &small);
        let lit = self.hover == Some(Button::Switch);
        let mid = b.min_y() + header_h / 2.0;
        let toggle = ds::switch(layer, &ds, right - px(36.0), mid, state.auto_upload(), lit);
        let label_x = toggle.min_x() - px(8.0) - label.width;
        let label_base = ds::optical_baseline(b.min_y(), header_h, &small);
        draw_text(layer, &label, label_x, label_base, if lit { p.text } else { p.text_muted });
        let switch = Rect::new(label_x, b.min_y(), toggle.max_x() - label_x, header_h);
        self.buttons.push((switch, Button::Switch));
        ds::divider_card(layer, &ds, b.min_x(), b.max_x(), b.min_y() + header_h);
        let list =
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, b.size.height - header_h);
        if self.rows.is_empty() {
            let empty = if self.generation == 0 {
                t("remote.pending_reading")
            } else {
                t("remote.pending_none")
            };
            ds::empty_state(layer, ctx.text, &ds, list, &empty, None);
            return;
        }
        let queries = self.queries.clone();
        self.tree.paint(
            ctx.scene,
            ctx.text,
            ctx.icons,
            &ds,
            (list, ctx.focused),
            queries.as_deref(),
        );
    }
}

#[cfg(test)]
#[path = "../tests/unit/pending_view.rs"]
mod tests;
