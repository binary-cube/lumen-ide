//! The HTTP tab (plan 77l), opened by the HTTP client's button: the tree of the project's
//! collections on the left ([`RequestTree`]: folders at any depth and their requests,
//! searched, its buttons for a new request, a new collection, the environments, an import),
//! the visual editor of the request chosen on the right ([`RequestEditor`], made when one is
//! chosen, let go with the tab), a separator between them that's dragged. One per project:
//! the button brings it forward. The project's file list it reads is asked of the app again
//! each time the tab is shown (it's a snapshot), and after what it writes itself.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Divider, NOWHERE, Press, Shown};
use ui_components::ds::{self, Ctx};
use ui_components::tool_panel::KeyReach;
use ui_components::{
    Orientation, PressMods, Splitter, SplitterStyle, ViewMenuEntry, Viewer, ViewerAction,
    ViewerCursor, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider, WebPage, WithHost,
};
use workbench_model::{ProjectQueries, WorkbenchHost};

use crate::editor::{EditorData, RequestEditor};
use crate::request_tree::{RequestTree, TreeAction};

/// The extension its tab is opened by.
pub const EXTENSION: &str = "view-http-collection";
/// The tree's width (logical pixels): at first, and the least either side keeps.
const TREE: f32 = 300.0;
const LEAST: f32 = 200.0;
const SEAM: f32 = 6.0;

/// What its tab is opened with: the project, the collection (a folder, relative), the request
/// to choose in it (its file, absolute, and its place).
#[derive(Clone)]
pub struct CollectionData {
    pub queries: Option<Arc<dyn ProjectQueries>>,
    pub project: Option<kernel::ProjectId>,
    pub collection: String,
    pub select: Option<(PathBuf, usize)>,
}

impl std::fmt::Debug for CollectionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectionData").field("collection", &self.collection).finish()
    }
}

/// The tree of the collection as a part of its tab: the tree's own input (its rows, its
/// search, a name being typed in it), its keys while it has them (Tab its own too); losing
/// them, its search stops and a name being typed is kept.
struct TreeSide<'a> {
    tree: Option<&'a mut RequestTree>,
    area: Rect,
}

impl Control for TreeSide<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }

    /// Held: the moves drag a request, the release activates a row.
    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if let Some(tree) = self.tree.as_deref_mut() {
            tree.press(p, clicks);
        }
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.tree.as_deref_mut().is_some_and(|t| t.drag(p))
    }

    fn release(&mut self) -> bool {
        self.tree.as_deref_mut().is_some_and(RequestTree::release)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.tree.as_deref_mut().is_some_and(|t| t.hover(p.unwrap_or(NOWHERE)))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.tree.as_deref()?.cursor_at(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.tree.as_deref_mut().is_some_and(|t| t.scroll(dy))
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn set_focus(&mut self, focused: bool) {
        if !focused && let Some(tree) = self.tree.as_deref_mut() {
            tree.focus_lost();
        }
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.tree.as_deref_mut().is_some_and(|t| t.edit_key(key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        self.tree.as_deref_mut().is_some_and(|t| t.text_input(typed))
    }

    fn selected_text(&self) -> Option<String> {
        self.tree.as_deref()?.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        let id = if id == "select-all" { "select_all" } else { id };
        self.tree.as_deref_mut().is_some_and(|t| t.edit_command(id))
    }

    fn tooltip(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        self.tree.as_deref()?.tooltip(p)
    }

    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.tree.as_deref_mut()?.menu_at(p, false)
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.tree.as_deref()?.frame_deadline()
    }
}

/// The editor of the request chosen as a part of its tab (nothing while none is): a tab's
/// view inside it, its press with the clicks and the modifiers, what it decided kept in
/// `action` for the tab; its keys while it has them (Tab its own too).
struct EditorSide<'a> {
    editor: Option<&'a mut RequestEditor>,
    area: Rect,
    action: &'a mut Option<ViewerAction>,
}

impl Control for EditorSide<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }

    /// Held: the moves select in its fields or drag its separator.
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        *self.action = self.editor.as_deref_mut().and_then(|e| e.pointer_down(p, clicks, mods));
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.drag(p))
    }

    fn release(&mut self) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.release())
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.hover(p.unwrap_or(NOWHERE)))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.editor.as_deref()?.cursor_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.scroll(dx, dy))
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.edit_key_with(key, reach))
    }

    /// What's typed or pasted (the editor takes both alike).
    fn text(&mut self, typed: &str) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.text_input(typed))
    }

    fn selected_text(&self) -> Option<String> {
        self.editor.as_deref()?.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.editor.as_deref_mut().is_some_and(|e| e.command(id))
    }

    fn tooltip(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        self.editor.as_deref()?.tooltip_at(p)
    }

    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.editor.as_deref_mut()?.context_menu(p)
    }

    fn animating(&self) -> bool {
        self.editor.as_deref().is_some_and(|e| e.is_animating())
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.editor.as_deref()?.frame_deadline()
    }
}

/// The tree, after the separator among its parts (see [`CollectionView::with_controls`]).
const TREE_SIDE: usize = 1;
/// The editor, last among its parts.
const EDITOR_SIDE: usize = 2;

#[derive(Default)]
pub struct CollectionView {
    tree: Option<RequestTree>,
    /// The editor of the request chosen, and which it is.
    editor: Option<(PathBuf, usize, RequestEditor)>,
    /// The tree's width (logical pixels).
    width: f32,
    splitter: Option<Splitter>,
    /// Where the held separator's line was dragged to (physical pixels across).
    split_at: Option<f32>,
    /// Its parts' state of use (see [`CollectionView::with_controls`]).
    controls: Controls,
    left: Rect,
    right: Rect,
    /// The menu shown is the tree's.
    tree_menu: bool,
    action: Option<ViewerAction>,
    /// What a press in the editor decided (the press hands it on).
    editor_action: Option<ViewerAction>,
    /// The project's queries asked again (the app's answer lands here), and the ask itself,
    /// before anything else it asks.
    fresh: Arc<Mutex<Option<Arc<dyn ProjectQueries>>>>,
    asking: Option<ViewerAction>,
    /// It asked the system's picker for a collection to import (the file chosen is its).
    importing: bool,
}

impl std::fmt::Debug for CollectionView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectionView").field("tree", &self.tree).finish_non_exhaustive()
    }
}

impl CollectionView {
    /// Its parts, the topmost first, with their state of use: the separator between the
    /// tree and the editor (once drawn), the tree, the editor. The keys are the tree's at
    /// first; a press gives them to the side pressed (a press on the separator leaves them),
    /// a request opened gives them to the editor. What its parts are told is noted as it
    /// goes (a part moving, the frame one wants), and what the tree asked is followed.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let answer = {
            let drawn = self.splitter.is_some();
            let mut unmade = Splitter::default();
            let splitter = self.splitter.as_mut().unwrap_or(&mut unmade);
            let mut divider = Divider { splitter, at: &mut self.split_at };
            let mut seam = Shown { part: &mut divider, shown: drawn };
            let mut tree = TreeSide { tree: self.tree.as_mut(), area: self.left };
            let mut editor = EditorSide {
                editor: self.editor.as_mut().map(|(_, _, e)| e),
                area: self.right,
                action: &mut self.editor_action,
            };
            let mut parts: [&mut dyn Control; 3] = [&mut seam, &mut tree, &mut editor];
            let controls = &mut self.controls;
            if controls.focused().is_none() {
                controls.focus(&mut parts, Some(TREE_SIDE));
            }
            let answer = f(controls, &mut parts);
            controls.settle(&parts);
            answer
        };
        self.take_tree();
        answer
    }

    /// The side with the keys is the editor's.
    fn editor_has_keys(&self) -> bool {
        self.controls.focused() == Some(EDITOR_SIDE)
    }

    /// Request `index` of `file` in the editor (the same: kept as it is), which has the keys.
    fn open_request(&mut self, file: PathBuf, index: usize) {
        self.give_editor_keys();
        if self.editor.as_ref().is_some_and(|(f, i, _)| *f == file && *i == index) {
            return;
        }
        let Some(tree) = &mut self.tree else { return };
        let root = tree.root();
        let data = EditorData {
            runs: crate::runs::Runs::of(tree.project_id(), root.as_deref()),
            file: file.clone(),
            index: Some(index),
        };
        // What the one before had changed is written first.
        if let Some((_, _, before)) = &mut self.editor {
            before.may_close();
        }
        let mut editor = RequestEditor::default();
        editor.set_data(&data);
        tree.set_current(Some((file.clone(), index)));
        self.editor = Some((file, index, editor));
    }

    /// The keys to the editor (from the tree: its search stops, a name typed is kept).
    fn give_editor_keys(&mut self) {
        if !self.editor_has_keys() {
            self.with_controls(|c, parts| c.focus(parts, Some(EDITOR_SIDE)));
        }
    }

    /// What the tree asked: a view's action passed on, a request opened beside it, an
    /// import.
    fn take_tree(&mut self) {
        let Some(action) = self.tree.as_mut().and_then(RequestTree::take_action) else { return };
        match action {
            TreeAction::View(action) => self.action = Some(action),
            TreeAction::Open(opened) => self.open_request(opened.file, opened.index),
            // Its folders open and close in place.
            TreeAction::OpenCollection(_) => {}
            TreeAction::Import => {
                let Some(root) = self.tree.as_ref().and_then(RequestTree::root) else { return };
                self.importing = true;
                self.action =
                    Some(ViewerAction::Request(ui_components::AppRequest::PickFile(Some(root))));
            }
            TreeAction::ImportCurl => self.import_curl(),
        }
    }

    /// The project's queries asked of the app again (a snapshot: its file list as it is now).
    fn ask_queries(&mut self) {
        let fresh = Arc::clone(&self.fresh);
        self.asking = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            *fresh.lock().unwrap_or_else(PoisonError::into_inner) = host.queries();
        })));
    }

    /// A file of requests made from the cURL command on the clipboard (one that isn't:
    /// said), then opened.
    fn import_curl(&mut self) {
        let Some(root) = self.tree.as_ref().and_then(RequestTree::root) else { return };
        if let Some(tree) = &self.tree {
            tree.refresh();
        }
        self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            let clipboard = host.clipboard_text().unwrap_or_default();
            let Some(request) = http_requests::curl::from_curl(&clipboard) else {
                host.notify(workbench_model::Notification::info(t("http.not_curl")));
                return;
            };
            let text = format!("### Imported from cURL\n{request}");
            host.spawn(Box::new(move || {
                let name =
                    crate::collections::free_name("imported", "", &|n| root.join(n).exists());
                let path = root.join(name);
                let written = files::write_atomic(&path, text.as_bytes()).is_ok();
                Box::new(move |host: &mut dyn WorkbenchHost| {
                    if written {
                        host.open_file(&path);
                    }
                })
            }));
        })));
    }

    /// The collection the picker gave, imported in the background, then opened.
    fn import(&mut self, picked: PathBuf) {
        let Some(root) = self.tree.as_ref().and_then(RequestTree::root) else { return };
        if let Some(tree) = &self.tree {
            tree.refresh();
        }
        self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            host.spawn(Box::new(move || {
                let made = crate::importing::import_file(&root, &picked);
                Box::new(move |host: &mut dyn WorkbenchHost| match made {
                    Ok(made) => {
                        host.open_file(&made.open);
                        host.notify(workbench_model::Notification::info(i18n::tr!(
                            "http.imported",
                            count = made.requests as i64
                        )));
                    }
                    Err(why) => host.notify(workbench_model::Notification::error(i18n::tr!(
                        "http.not_imported",
                        why = why.as_str()
                    ))),
                })
            }));
        })));
    }

    fn editor(&mut self) -> Option<&mut RequestEditor> {
        self.editor.as_mut().map(|(_, _, e)| e)
    }
}

impl Viewer for CollectionView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<CollectionData>() else { return };
        if self.tree.as_ref().is_none_or(|t| t.within() != data.collection) {
            self.tree = Some(RequestTree::collection(&data.collection));
            self.editor = None;
        }
        if let Some(tree) = &mut self.tree {
            tree.set_project(data.queries.clone(), data.project);
        }
        if let Some((file, index)) = data.select.clone() {
            self.open_request(file, index);
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, waker, focused } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        if self.width <= 0.0 {
            self.width = TREE;
        }
        let most = (b.size.width / scale - LEAST).max(LEAST);
        let width = px(self.width.clamp(LEAST, most)).min(b.size.width);
        self.left = Rect::new(b.min_x(), b.min_y(), width, b.size.height);
        let seam = px(SEAM);
        self.right = Rect::new(
            self.left.max_x() + seam / 2.0,
            b.min_y(),
            (b.size.width - width - seam / 2.0).max(0.0),
            b.size.height,
        );
        let editor_keys = self.editor_has_keys();
        if let Some(tree) = &mut self.tree {
            tree.paint(ViewerPaint {
                bounds: self.left,
                scale,
                text: &mut *text,
                scene: &mut *scene,
                palette: p,
                icons,
                waker: waker.clone(),
                focused: focused && !editor_keys,
            });
        }
        let layer = scene.push_layer(Some(self.right));
        layer.quad(Quad::filled(self.right, p.editor_background));
        match &mut self.editor {
            Some((_, _, editor)) => editor.paint(ViewerPaint {
                bounds: self.right,
                scale,
                text: &mut *text,
                scene: &mut *scene,
                palette: p,
                icons,
                waker,
                focused: focused && editor_keys,
            }),
            None => ds::empty_state(
                layer,
                text,
                &ds,
                self.right,
                &t("http.pick_request"),
                Some(&t("http.pick_request_detail")),
            ),
        }
        let splitter = self.splitter.get_or_insert_with(|| {
            Splitter::new(SplitterStyle::from_palette(p), Orientation::Vertical)
        });
        splitter.set_style(SplitterStyle::from_palette(p));
        splitter.set_bounds(
            Rect::new(self.left.max_x() - seam / 2.0, b.min_y(), seam, b.size.height),
            scale,
        );
        splitter.paint(scene);
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn set_shown(&mut self, shown: bool) {
        // Seen again: the project's files as they are now.
        if shown {
            self.ask_queries();
        }
    }

    fn poll(&mut self) -> bool {
        let fresh = self.fresh.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let (Some(queries), Some(tree)) = (fresh, &mut self.tree) {
            let project = tree.project_id();
            tree.set_project(Some(queries), project);
        }
        // It wrote something: the project's file list asked again too (a file outside the
        // collections' folder isn't read from the disk).
        if self.tree.as_ref().is_some_and(RequestTree::is_stale) && self.asking.is_none() {
            self.ask_queries();
        }
        let mut changed = self.tree.as_mut().is_some_and(RequestTree::poll);
        self.take_tree();
        if let Some(editor) = self.editor() {
            changed |= editor.poll();
        }
        // What its parts want of the frames now (a write due, a fade).
        self.with_controls(|c, parts| c.settle(parts));
        changed
    }

    /// Its parts' soonest frame (the tree's scrollbar fading, the editor's write due), as
    /// noted at their last input, paint or poll.
    fn frame_deadline(&self) -> Option<Instant> {
        self.controls.next_frame()
    }

    fn is_loading(&self) -> bool {
        self.editor.as_ref().is_some_and(|(_, _, e)| e.is_loading())
    }

    /// The separator or the editor's moving.
    fn is_animating(&self) -> bool {
        self.controls.is_animating()
    }

    /// The wheel to the side under the mouse.
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some()
    }

    /// The side pressed takes the press (and the keys); the separator is held.
    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press_with(parts, p, clicks, mods));
        self.editor_action.take().or_else(|| self.action.take())
    }

    fn drag(&mut self, p: Point) -> bool {
        let moved = self.with_controls(|c, parts| c.drag(parts, p));
        // The separator held: the tree's width follows it.
        if let Some(at) = self.split_at.take() {
            let scale = if self.left.size.width > 0.0 && self.width > 0.0 {
                (self.left.size.width / self.width.max(1.0)).max(0.1)
            } else {
                1.0
            };
            self.width = ((at - self.left.min_x()) / scale).max(LEAST);
            return true;
        }
        moved.is_some_and(|(_, changed)| changed)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.controls.cursor_at(p)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        self.controls.tooltip_at(p)
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    /// The side with the keys wants the text typed.
    fn wants_text(&self) -> bool {
        if self.editor_has_keys() {
            self.editor.as_ref().is_some_and(|(_, _, e)| e.wants_text())
        } else {
            self.tree.as_ref().is_some_and(RequestTree::wants_text)
        }
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    /// What the side with the keys has selected.
    fn selected_text(&self) -> Option<String> {
        if self.editor_has_keys() {
            self.editor.as_ref()?.2.selected_text()
        } else {
            self.tree.as_ref()?.selected_text()
        }
    }

    fn select_all(&mut self) -> bool {
        self.with_controls(|c, parts| c.command(parts, "select_all")).is_some()
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.with_controls(|c, parts| c.key(parts, key, reach)).is_some()
    }

    /// The menu of the part under the mouse (its rows come back to it as commands).
    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.tree_menu = false;
        let (part, menu) = self.with_controls(|c, parts| c.menu(parts, p))?;
        self.tree_menu = part == TREE_SIDE;
        Some(menu)
    }

    fn command(&mut self, id: &str) -> bool {
        // The system's picker: its file (asked by the Import), or none.
        if id == "picked-file-cancelled" {
            self.importing = false;
            return false;
        }
        if let Some(picked) = id.strip_prefix("picked-file:") {
            if !std::mem::take(&mut self.importing) {
                return false;
            }
            self.import(PathBuf::from(picked));
            return true;
        }
        if std::mem::take(&mut self.tree_menu)
            && self.tree.as_mut().is_some_and(|t| t.menu_command(id))
        {
            self.take_tree();
            return true;
        }
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        if let Some(asking) = self.asking.take() {
            return Some(asking);
        }
        self.take_tree();
        if let Some(action) = self.action.take() {
            return Some(action);
        }
        self.editor()?.take_action()
    }

    fn web_page(&self) -> Option<WebPage> {
        self.editor.as_ref()?.2.web_page()
    }

    fn may_close(&mut self) -> bool {
        self.editor().is_none_or(|e| e.may_close())
    }
}

/// The collection's tab.
#[derive(Debug, Default)]
pub struct CollectionProvider;

impl ViewerProvider for CollectionProvider {
    fn id(&self) -> &'static str {
        "http-collection"
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(CollectionView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/collection_view.rs"]
mod tests;
