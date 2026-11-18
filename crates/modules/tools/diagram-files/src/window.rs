//! The Diagrams window (a side panel): the project's diagrams — the `.drawio` files kept in
//! `<project dir>/diagrams/`, in their folders (real folders on disk: Git and draw.io see
//! them) — listed on a worker (bounded: depth, count, no links followed), read again when
//! something changes in that folder (the disk's watcher hears it as the project's: the
//! module says it's its own). A diagram clicked opens as a tab; a folder clicked folds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::tree::{EntryIcon, EntryLayout, entry_icon};
use ui_components::{PanelButton, PanelContext, ToolPanel, ToolPanelAction, ViewerPaint, ds};
use ui_kit::draw_text;
use workbench_model::ProjectView;

use crate::window_actions::Done;

type Waker = Arc<dyn Fn() + Send + Sync>;

/// The folder in the project's own folder where diagrams are kept.
pub const FOLDER: &str = "diagrams";
/// How deep folders are listed, and how many entries at most.
const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 5000;
/// A row's height and a level's indentation (logical pixels).
const ROW_H: f32 = 28.0;
const INDENT: f32 = 16.0;

/// An entry listed: a folder or a diagram, at its depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
}

/// `dir`'s folders and diagrams (folders first, each level by name, case aside); none if
/// it isn't there. Links aren't followed; at most so deep, so many.
pub fn list(dir: &Path) -> Vec<Entry> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<Entry>) {
        if depth >= MAX_DEPTH || out.len() >= MAX_ENTRIES {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut here: Vec<(bool, String, PathBuf)> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let kind = e.file_type().ok()?;
                let name = e.file_name().to_string_lossy().into_owned();
                if kind.is_symlink() || name.starts_with('.') {
                    return None;
                }
                let diagram = kind.is_file()
                    && Path::new(&name)
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case(crate::EXTENSION));
                (kind.is_dir() || diagram).then(|| (kind.is_dir(), name, e.path()))
            })
            .collect();
        here.sort_by_cached_key(|(is_dir, name, _)| (!*is_dir, name.to_lowercase()));
        for (is_dir, name, path) in here {
            if out.len() >= MAX_ENTRIES {
                return;
            }
            out.push(Entry { path: path.clone(), name, depth, is_dir });
            if is_dir {
                walk(&path, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

/// The panel.
#[derive(Default)]
pub struct DiagramsPanel {
    /// The project's diagrams folder (`None`: no project).
    pub(crate) dir: Option<PathBuf>,
    pub(crate) entries: Vec<Entry>,
    /// Folded folders.
    folded: HashSet<PathBuf>,
    /// The worker's listing, taken at the next poll.
    listed: Arc<Mutex<Option<Vec<Entry>>>>,
    /// Something changed in the folder: listed again at the next poll.
    stale: Arc<AtomicBool>,
    listing: bool,
    /// Shown, not listed yet (its first paint lists it).
    unlisted: bool,
    pub(crate) waker: Arc<Mutex<Option<Waker>>>,
    subscriptions: Vec<kernel::Subscription>,
    /// The rows drawn: where, which entry.
    rows: Vec<(Rect, usize)>,
    /// The project's root and its events (what's deleted here is told there).
    pub(crate) root: Option<PathBuf>,
    pub(crate) events: Option<kernel::EventBus>,
    /// Where something new goes: the folder last clicked (or what a file clicked is in).
    pub(crate) current: Option<PathBuf>,
    /// An entry being renamed in its row.
    pub(crate) renaming: Option<(PathBuf, ui_kit::TextInput)>,
    /// The entry a menu was opened on.
    pub(crate) menu_target: Option<PathBuf>,
    /// What the worker made or did (a diagram made: opened), taken at the next poll.
    pub(crate) done: Arc<Mutex<Vec<Done>>>,
    /// What's asked of the app (a diagram opened, an entry renamed or moved).
    pub(crate) action: Option<ToolPanelAction>,
    hovered: Option<usize>,
    /// A row pressed (its entry, where): a click on release, a drag once it moves.
    pressed: Option<(PathBuf, Point)>,
    /// An entry being dragged: where the mouse is and the folder it would go into.
    dragging: Option<Dragging>,
    scroll: f32,
    content_h: f32,
    bounds: Rect,
    content_scale: f32,
}

/// An entry being dragged (see [`DiagramsPanel::drag_to`]).
#[derive(Debug, Clone)]
pub(crate) struct Dragging {
    pub(crate) path: PathBuf,
    pub(crate) at: Point,
    /// The folder it would go into (`None`: nowhere it can go — itself, where it is).
    pub(crate) into: Option<PathBuf>,
}

/// How far the mouse moves before a press becomes a drag (logical pixels).
const DRAG_START: f32 = 4.0;

impl std::fmt::Debug for DiagramsPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramsPanel")
            .field("dir", &self.dir)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl DiagramsPanel {
    /// Shows the diagrams of folder `dir` (listed on the worker).
    pub fn show(&mut self, dir: PathBuf) {
        if self.dir.as_ref() == Some(&dir) {
            return;
        }
        self.dir = Some(dir);
        self.entries.clear();
        self.folded.clear();
        // Listed at its first paint, once it can wake the app (never here, on the UI thread).
        self.unlisted = true;
    }

    /// The folder listed on the worker; without a waker (tests) here and now.
    pub(crate) fn list(&mut self) {
        let Some(dir) = self.dir.clone() else { return };
        self.listing = true;
        let (into, waker) = (Arc::clone(&self.listed), Arc::clone(&self.waker));
        let job = move || {
            let entries = list(&dir);
            *into.lock().unwrap_or_else(PoisonError::into_inner) = Some(entries);
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                wake();
            }
        };
        if self.waker.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            job();
        } else {
            kernel::work::run("diagram-list", job);
        }
    }

    /// The entries shown: those whose folders aren't folded.
    fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut hidden_below: Option<usize> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if let Some(depth) = hidden_below {
                if e.depth > depth {
                    continue;
                }
                hidden_below = None;
            }
            out.push(i);
            if e.is_dir && self.folded.contains(&e.path) {
                hidden_below = Some(e.depth);
            }
        }
        out
    }

    pub(crate) fn entry_at(&self, p: Point) -> Option<usize> {
        self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i)
    }

    /// The pressed entry dragged to `p` (past a few pixels): the folder under the mouse
    /// (a diagram's own folder, the diagrams folder below the rows) is where it would go —
    /// never into itself, nor where it already is. `true`: a redraw.
    fn drag_to(&mut self, p: Point) -> bool {
        let Some((path, from)) = self.pressed.clone() else { return false };
        let scale = (self.content_scale).max(1.0);
        if self.dragging.is_none()
            && (p.x - from.x).abs().max((p.y - from.y).abs()) < DRAG_START * scale
        {
            return false;
        }
        let into = match self.entry_at(p).and_then(|i| self.entries.get(i)) {
            Some(e) if e.is_dir => Some(e.path.clone()),
            Some(e) => e.path.parent().map(Path::to_path_buf),
            None if self.bounds.contains(p) => self.dir.clone(),
            None => None,
        };
        let into =
            into.filter(|dir| !dir.starts_with(&path) && path.parent() != Some(dir.as_path()));
        self.renaming = None;
        self.dragging = Some(Dragging { path, at: p, into });
        true
    }
}

impl ToolPanel for DiagramsPanel {
    fn update(&mut self, _ctx: &PanelContext<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(view) = data.downcast_ref::<ProjectView>() else { return };
        let Some(root) = view.queries.as_ref().map(|q| q.root()) else { return };
        let dir = root.join(kernel::brand::PROJECT_DIR).join(FOLDER);
        self.root = Some(root);
        self.events = Some(view.events.clone());
        // What changes in the folder (made, removed, renamed): listed again.
        let (stale, waker, inside) =
            (Arc::clone(&self.stale), Arc::clone(&self.waker), dir.clone());
        self.subscriptions =
            vec![view.events.subscribe(move |e: &workspace::events::EntriesChanged| {
                if e.paths.iter().any(|p| p.starts_with(&inside) || inside.starts_with(p)) {
                    stale.store(true, Ordering::Release);
                    if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone()
                    {
                        wake();
                    }
                }
            })];
        self.show(dir);
    }

    fn poll(&mut self) -> bool {
        if self.stale.swap(false, Ordering::AcqRel) {
            self.list();
        }
        self.take_done();
        let Some(entries) = self.listed.lock().unwrap_or_else(PoisonError::into_inner).take()
        else {
            return false;
        };
        self.listing = false;
        self.entries = entries;
        self.hovered = None;
        true
    }

    /// New Diagram, New Folder (in the folder last clicked).
    fn header(&self) -> Option<Vec<PanelButton>> {
        let button = |id, icon, key: &str| PanelButton { id, icon, tooltip: t(key), group: 0 };
        Some(vec![
            button("new-diagram", "plus", "diagram_files.new_diagram"),
            button("new-folder", "folder-plus", "diagram_files.new_folder"),
        ])
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            "new-diagram" => self.create(false),
            "new-folder" => self.create(true),
            _ => {}
        }
        None
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    fn menu_at(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.row_menu(p)
    }

    fn menu_command(&mut self, id: &str) -> bool {
        self.row_command(id)
    }

    fn wants_text(&self) -> bool {
        self.renaming.is_some()
    }

    fn text_input(&mut self, text: &str) -> bool {
        let Some((_, input)) = self.renaming.as_mut() else { return false };
        input.insert(text.lines().next().unwrap_or_default());
        true
    }

    fn edit_key(&mut self, key: ui_components::ViewerKey) -> bool {
        self.rename_key(key)
    }

    fn focus_lost(&mut self) {
        self.renaming = None;
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // Listed before the waker was known (its first paint): now it can wake.
        if let (Some(w), Ok(mut slot)) = (&ctx.waker, self.waker.lock())
            && slot.is_none()
        {
            *slot = Some(Arc::clone(w));
        }
        if std::mem::take(&mut self.unlisted) {
            self.list();
        }
        self.bounds = ctx.bounds;
        self.content_scale = ctx.scale;
        let ds = ds::Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        let font = ds.ui_font(0.0);
        self.rows.clear();
        let visible = self.visible();
        if visible.is_empty() {
            if self.dir.is_some() && !self.listing {
                let lines = [
                    (t("diagram_files.none"), p.text_muted),
                    (
                        format!("{}/{FOLDER}/", kernel::brand::PROJECT_DIR),
                        p.text_muted.with_alpha(0.7),
                    ),
                ];
                let mut y = ctx.bounds.min_y() + ds.px(24.0);
                for (line, color) in lines {
                    let shaped = ctx.text.shape_line(&line, &font);
                    let x = (ctx.bounds.center().x - shaped.width / 2.0).round();
                    draw_text(layer, &shaped, x, (y + font.size).round(), color);
                    y += font.size * 1.6;
                }
            }
            return;
        }
        let layout = EntryLayout::new(ctx.scale, ROW_H, INDENT);
        let row_h = ds.px(ROW_H);
        self.content_h = row_h * visible.len() as f32;
        let max_scroll = (self.content_h - ctx.bounds.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let diagram = ctx.icons.id("diagram").unwrap_or_else(|| ctx.icons.id_or_file("file"));
        for (n, &i) in visible.iter().enumerate() {
            let top = ctx.bounds.min_y() + row_h * n as f32 - self.scroll;
            if top + row_h < ctx.bounds.min_y() || top > ctx.bounds.max_y() {
                continue;
            }
            let row = Rect::new(ctx.bounds.min_x(), top, ctx.bounds.size.width, row_h);
            self.rows.push((row, i));
            let e = &self.entries[i];
            // Where what's dragged would go: lit (the hover would mislead meanwhile).
            let target = self.dragging.as_ref().and_then(|d| d.into.as_ref()) == Some(&e.path);
            if target {
                layer.quad(
                    Quad::filled(layout.pill(ctx.bounds, top), p.accent.with_alpha(0.12))
                        .rounded(layout.pill_radius())
                        .bordered(ds.line(), p.accent.with_alpha(0.6)),
                );
            } else if self.hovered == Some(i) && self.dragging.is_none() {
                layer.quad(
                    Quad::filled(layout.pill(ctx.bounds, top), p.tree_hover)
                        .rounded(layout.pill_radius()),
                );
            }
            let x = layout.x(ctx.bounds.min_x(), e.depth);
            let entry = if e.is_dir {
                EntryIcon::Folder {
                    name: &e.name,
                    expanded: !self.folded.contains(&e.path),
                    color: p.folder_icon,
                }
            } else {
                EntryIcon::File { icon: diagram, color: p.accent }
            };
            let icon =
                entry_icon(layer, ctx.icons, &layout, (x, top), entry, (p.text_muted, &|c| c));
            let label_x = layout.label_x(icon);
            // A diagram by its name without `.drawio`.
            let name =
                if e.is_dir { e.name.as_str() } else { crate::window_actions::stem(&e.name) };
            let room = (ctx.bounds.max_x() - label_x - ds.px(8.0)).max(0.0);
            // Being renamed: its field in the label's place.
            if let Some((_, input)) = self.renaming.as_ref().filter(|(at, _)| *at == e.path) {
                let field = Rect::new(
                    label_x - ds.px(4.0),
                    top + ds.px(3.0),
                    room + ds.px(4.0),
                    row_h - ds.px(6.0),
                );
                let style = crate::window_actions::field_style(ctx.palette, &ds, font.clone());
                input.paint(layer, ctx.text, field, true, "", &style);
                continue;
            }
            let label = ds::ellipsize(ctx.text, name, &font, room);
            let shaped = ctx.text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                label_x,
                (top + row_h / 2.0 + font.size * 0.35).round(),
                p.text,
            );
        }
        // What's dragged follows the mouse, over the rows.
        if let Some(drag) = &self.dragging
            && let Some(e) = self.entries.iter().find(|e| e.path == drag.path)
        {
            let (icon, color) =
                if e.is_dir { ("folder", p.folder_icon) } else { ("diagram", p.accent) };
            let name =
                if e.is_dir { e.name.as_str() } else { crate::window_actions::stem(&e.name) };
            let top = ctx.scene.push_layer(Some(ctx.bounds));
            let lead = ui_components::drag_card::Lead::Icon(ctx.icons.id_or_file(icon), color);
            ui_components::drag_card::paint(top, &ds, ctx.text, ctx.icons, lead, name, drag.at);
        }
    }

    fn scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let max = (self.content_h - self.bounds.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        let e = self.entries.get(self.entry_at(p)?)?.clone();
        self.renaming = None;
        let parent = e.path.parent().map(Path::to_path_buf).unwrap_or_default();
        self.current = Some(if e.is_dir { e.path.clone() } else { parent });
        if e.is_dir {
            if !self.folded.remove(&e.path) {
                self.folded.insert(e.path);
            }
            return None;
        }
        Some(ToolPanelAction::Open { path: e.path, line: 0 })
    }

    /// A row: a click once released (a drag if it moves first).
    fn press(&mut self, p: Point, _clicks: u32) -> Option<ToolPanelAction> {
        match self.entry_at(p).and_then(|i| self.entries.get(i)) {
            Some(e) => {
                self.pressed = Some((e.path.clone(), p));
                None
            }
            None => self.click(p),
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        self.drag_to(p)
    }

    fn release(&mut self) -> bool {
        let pressed = self.pressed.take();
        if let Some(drag) = self.dragging.take() {
            if let Some(into) = drag.into {
                self.action = Some(ToolPanelAction::Move { from: drag.path, into });
            }
            return true;
        }
        let Some((_, at)) = pressed else { return false };
        if let Some(action) = self.click(at) {
            self.action = Some(action);
        }
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let at = self.entry_at(p);
        std::mem::replace(&mut self.hovered, at) != at
    }

    fn clickable(&self, p: Point) -> bool {
        self.entry_at(p).is_some()
    }
}

#[cfg(test)]
#[path = "../tests/unit/window.rs"]
mod tests;
