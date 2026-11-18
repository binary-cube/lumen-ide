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

/// Which side has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Side {
    #[default]
    Tree,
    Editor,
}

#[derive(Default)]
pub struct CollectionView {
    tree: Option<RequestTree>,
    /// The editor of the request chosen, and which it is.
    editor: Option<(PathBuf, usize, RequestEditor)>,
    /// The tree's width (logical pixels).
    width: f32,
    splitter: Option<Splitter>,
    left: Rect,
    right: Rect,
    side: Side,
    /// Where the mouse is (the wheel goes there), and which side a press began on.
    over: Side,
    pressed: Option<Side>,
    /// The menu shown is the tree's.
    tree_menu: bool,
    action: Option<ViewerAction>,
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
    /// Request `index` of `file` in the editor (the same: kept as it is).
    fn open_request(&mut self, file: PathBuf, index: usize) {
        self.side = Side::Editor;
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

    fn side_at(&self, p: Point) -> Option<Side> {
        if self.left.contains(p) {
            Some(Side::Tree)
        } else if self.right.contains(p) {
            Some(Side::Editor)
        } else {
            None
        }
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
        if let Some(tree) = &mut self.tree {
            tree.paint(ViewerPaint {
                bounds: self.left,
                scale,
                text: &mut *text,
                scene: &mut *scene,
                palette: p,
                icons,
                waker: waker.clone(),
                focused: focused && self.side == Side::Tree,
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
                focused: focused && self.side == Side::Editor,
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
        changed
    }

    fn frame_deadline(&self) -> Option<Instant> {
        let tree = self.tree.as_ref().and_then(RequestTree::frame_deadline);
        let editor = self.editor.as_ref().and_then(|(_, _, e)| e.frame_deadline());
        tree.into_iter().chain(editor).min()
    }

    fn is_loading(&self) -> bool {
        self.editor.as_ref().is_some_and(|(_, _, e)| e.is_loading())
    }

    fn is_animating(&self) -> bool {
        self.splitter.as_ref().is_some_and(Splitter::is_animating)
            || self.editor.as_ref().is_some_and(|(_, _, e)| e.is_animating())
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        match self.over {
            Side::Tree => self.tree.as_mut().is_some_and(|t| t.scroll(dy)),
            Side::Editor => self.editor().is_some_and(|e| e.scroll(dx, dy)),
        }
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        if self.splitter.as_mut().is_some_and(|s| s.begin_drag(p)) {
            return None;
        }
        let side = self.side_at(p)?;
        self.pressed = Some(side);
        match side {
            Side::Tree => {
                self.side = Side::Tree;
                if let Some(tree) = &mut self.tree {
                    tree.press(p, clicks);
                }
                self.take_tree();
                self.action.take()
            }
            Side::Editor => {
                // The keyboard left the tree: a name typed there is kept.
                if self.side == Side::Tree {
                    if let Some(tree) = &mut self.tree {
                        tree.focus_lost();
                    }
                    self.take_tree();
                }
                self.side = Side::Editor;
                self.editor().and_then(|e| e.pointer_down(p, clicks, mods))
            }
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(at) = self.splitter.as_ref().and_then(|s| s.drag_to(p)) {
            let scale = if self.left.size.width > 0.0 && self.width > 0.0 {
                (self.left.size.width / self.width.max(1.0)).max(0.1)
            } else {
                1.0
            };
            self.width = ((at - self.left.min_x()) / scale).max(LEAST);
            return true;
        }
        match self.pressed {
            Some(Side::Tree) => self.tree.as_mut().is_some_and(|t| t.drag(p)),
            Some(Side::Editor) => self.editor().is_some_and(|e| e.drag(p)),
            None => false,
        }
    }

    fn release(&mut self) -> bool {
        if self.splitter.as_mut().is_some_and(Splitter::end_drag) {
            return true;
        }
        let released = match self.pressed.take() {
            Some(Side::Tree) => self.tree.as_mut().is_some_and(RequestTree::release),
            Some(Side::Editor) => self.editor().is_some_and(|e| e.release()),
            None => false,
        };
        self.take_tree();
        released
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.splitter.as_mut().is_some_and(|s| s.hover(p));
        if let Some(side) = self.side_at(p) {
            self.over = side;
        }
        changed |= self.tree.as_mut().is_some_and(|t| t.hover(p));
        if let Some(editor) = self.editor() {
            changed |= editor.hover(p);
        }
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.splitter.as_ref().is_some_and(|s| s.contains(p)) {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        match self.side_at(p)? {
            Side::Tree => self.tree.as_ref()?.cursor_at(p),
            Side::Editor => self.editor.as_ref()?.2.cursor_at(p),
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        match self.side_at(p)? {
            Side::Tree => self.tree.as_ref()?.tooltip(p),
            Side::Editor => self.editor.as_ref()?.2.tooltip_at(p),
        }
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn wants_text(&self) -> bool {
        match self.side {
            Side::Tree => self.tree.as_ref().is_some_and(RequestTree::wants_text),
            Side::Editor => self.editor.as_ref().is_some_and(|(_, _, e)| e.wants_text()),
        }
    }

    fn text_input(&mut self, typed: &str) -> bool {
        match self.side {
            Side::Tree => self.tree.as_mut().is_some_and(|t| t.text_input(typed)),
            Side::Editor => self.editor().is_some_and(|e| e.text_input(typed)),
        }
    }

    fn paste(&mut self, text: &str) -> bool {
        match self.side {
            Side::Tree => self.tree.as_mut().is_some_and(|t| t.text_input(text)),
            Side::Editor => self.editor().is_some_and(|e| e.paste(text)),
        }
    }

    fn selected_text(&self) -> Option<String> {
        match self.side {
            Side::Tree => self.tree.as_ref()?.selected_text(),
            Side::Editor => self.editor.as_ref()?.2.selected_text(),
        }
    }

    fn select_all(&mut self) -> bool {
        match self.side {
            Side::Tree => self.tree.as_mut().is_some_and(RequestTree::select_all),
            Side::Editor => self.editor().is_some_and(|e| e.select_all()),
        }
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = match self.side {
            Side::Tree => self.tree.as_mut().is_some_and(|t| t.edit_key(key, reach)),
            Side::Editor => self.editor().is_some_and(|e| e.edit_key_with(key, reach)),
        };
        self.take_tree();
        used
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.tree_menu = false;
        match self.side_at(p)? {
            Side::Tree => {
                self.tree_menu = true;
                self.tree.as_mut()?.menu_at(p, false)
            }
            Side::Editor => self.editor()?.context_menu(p),
        }
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
        match self.side {
            Side::Tree if id == "select_all" || id == "select-all" => self.select_all(),
            Side::Tree => false,
            Side::Editor => self.editor().is_some_and(|e| e.command(id)),
        }
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
