//! The recordings' tab: every recording kept (the app's data folder, `profiles/`), the
//! newest first, as the shared table shows rows; a filter over them (its date or its
//! name); a click opens one; a right-click offers Open, Copy Summary (its findings and
//! figures as Markdown, read on a worker), Show in Folder and Delete (asked first, moved to
//! the trash). The folder is read on a worker (`profiler-read`), again after a recording or
//! a deletion; nothing is kept but the rows shown.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::SystemTime;

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Press, Thumb};
use ui_components::ds::{self, Ctx};
use ui_components::search_field::SearchField;
use ui_components::{
    AppRequest, EdgeScrollbar, KeyReach, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor,
    ViewerKey, ViewerPaint, WithHost,
};

use crate::view::page::{Cell, Col, Page};
use crate::view::words;

/// The extension the tab is opened by.
pub const EXTENSION: &str = "view-profiler-recordings";
/// The recordings listed at most (the newest).
const MOST: usize = 500;
/// The filter's place among its parts (see [`RecordingsView::with_controls`]).
const FILTER: usize = 1;

/// The edit command choosing a whole field (it changes nothing listed).
const SELECT_ALL: &str = "select_all";

/// The filter field's height (logical pixels).
const FIELD_H: f32 = 34.0;

/// A recording as listed: its file, when it was made, its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub path: PathBuf,
    pub name: String,
    pub when: String,
    pub size: u64,
}

/// The recordings in `folder`, the newest first, at most [`MOST`] (files only, named as a
/// recording is).
pub fn listed(folder: &Path) -> Vec<Listed> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut rows: Vec<(SystemTime, Listed)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !crate::view::is_recording(&name) {
                return None;
            }
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let when = when_made(&name);
            Some((modified, Listed { path: e.path(), name, when, size: meta.len() }))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.name.cmp(&a.1.name)));
    rows.into_iter().take(MOST).map(|(_, l)| l).collect()
}

/// When a recording was made, from its name (its moment in UTC) in the user's zone; empty
/// for a name without one.
pub fn when_made(name: &str) -> String {
    let stamp = name.trim_start_matches("profile-").get(..15).unwrap_or_default();
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").map_or_else(
        |_| String::new(),
        |at| at.and_utc().with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// The tab's view.
pub struct RecordingsView {
    rows: Option<Vec<Listed>>,
    asked: Option<Receiver<Vec<Listed>>>,
    /// A summary being read for the clipboard.
    copying: Option<Receiver<Option<String>>>,
    /// The recordings' count last seen written (a new one: listed again).
    seen: u64,
    /// The filter (the app's search field: ×, ⌘A; its focus the parts').
    filter: SearchField,
    /// The rows as drawn (their place in the filtered list), the one under the mouse, the
    /// one a menu was opened on.
    drawn: Vec<(Rect, usize)>,
    lit: Option<usize>,
    /// The recording its open menu is for (by its file: the rows may change under it).
    menu_on: Option<PathBuf>,
    scroll: f32,
    content_h: f32,
    view_h: f32,
    bar: EdgeScrollbar,
    /// Its parts' state of use (see [`RecordingsView::with_controls`]).
    controls: Controls,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    action: Option<ViewerAction>,
}

impl std::fmt::Debug for RecordingsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingsView").finish_non_exhaustive()
    }
}

impl Default for RecordingsView {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingsView {
    pub fn new() -> Self {
        Self {
            rows: None,
            asked: None,
            copying: None,
            seen: crate::recording::written(),
            filter: SearchField::new(),
            drawn: Vec::new(),
            lit: None,
            menu_on: None,
            scroll: 0.0,
            content_h: 0.0,
            view_h: 0.0,
            bar: EdgeScrollbar::default(),
            controls: Controls::default(),
            waker: None,
            action: None,
        }
    }

    /// Its parts (the page's scrollbar with its offset, the filter at [`FILTER`]), the
    /// topmost first,
    /// with their state of use: the one with the focus has the keys.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut page = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut filter = FilterField { field: &mut self.filter };
        let mut parts: [&mut dyn Control; 2] = [&mut page, &mut filter];
        f(&mut self.controls, &mut parts)
    }

    /// A part has the keys (the filter: the only one that takes them).
    fn typing(&self) -> bool {
        self.controls.focused().is_some()
    }

    /// The folder read again, on a worker.
    fn read(&mut self) {
        let Some(folder) = crate::recording::folder() else {
            self.rows = Some(Vec::new());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let wake = self.waker.clone();
        kernel::work::run(crate::view::THREAD, move || {
            let _ = tx.send(listed(&folder));
            if let Some(wake) = wake {
                wake();
            }
        });
        self.asked = Some(rx);
    }

    fn take_read(&mut self) -> bool {
        let mut changed = false;
        if let Some(rows) = self.asked.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.asked = None;
            self.rows = Some(rows);
            changed = true;
        }
        if let Some(copied) = self.copying.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.copying = None;
            if let Some(text) = copied {
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            changed = true;
        }
        // A recording written meanwhile, or one deleted: listed again.
        let written = crate::recording::written();
        if written != self.seen && self.asked.is_none() {
            self.seen = written;
            self.read();
        }
        changed
    }

    /// The rows the filter leaves (by date or name, any case).
    fn shown(&self) -> Vec<&Listed> {
        let query = self.filter.text().trim().to_lowercase();
        self.rows
            .iter()
            .flatten()
            .filter(|r| {
                query.is_empty()
                    || r.when.contains(&query)
                    || r.name.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn row_at(&self, p: Point) -> Option<usize> {
        self.drawn.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i)
    }

    fn path_of(&self, row: usize) -> Option<PathBuf> {
        self.shown().get(row).map(|r| r.path.clone())
    }

    /// Recording `row`'s summary read on a worker, then copied.
    fn copy_summary(&mut self, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let wake = self.waker.clone();
        kernel::work::run(crate::view::THREAD, move || {
            let text = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| profiling::Recording::from_json(&t).ok())
                .map(|r| words::as_text(&r, &profiling::summary(&r)));
            let _ = tx.send(text);
            if let Some(wake) = wake {
                wake();
            }
        });
        self.copying = Some(rx);
    }

    /// Recording `row` deleted (moved to the trash), asked first.
    fn delete(&mut self, path: PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let question = workbench_model::Question {
            title: t("profiler.delete_title"),
            message: t("profiler.delete_message"),
            icon: "trash",
            yes: t("profiler.delete"),
            danger: true,
            list: vec![name],
        };
        self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            host.confirm(
                question,
                Box::new(move |host| {
                    host.spawn(Box::new(move || {
                        let moved = fs_local::move_to_trash(std::slice::from_ref(&path));
                        if let Err(e) = &moved {
                            tracing::warn!(error = %e, "a profile recording couldn't be deleted");
                        }
                        crate::recording::touched();
                        Box::new(|_host: &mut dyn workbench_model::WorkbenchHost| {})
                    }));
                }),
            );
        })));
    }
}

impl Viewer for RecordingsView {
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        if self.rows.is_none() && self.asked.is_none() {
            self.read();
        }
        self.take_read();
        let ViewerPaint { bounds: b, scale, text, scene, palette, icons, focused, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        scene.push_layer(Some(b)).quad(Quad::filled(b, palette.editor_background));
        let room = (b.size.width - 2.0 * ds.px(crate::view::page::PAD) - ds.px(20.0)).max(0.0);
        let width = room.min(ds.px(crate::view::page::MAX_W));
        let x = b.min_x() + ds.px(crate::view::page::PAD) + ((room - width) / 2.0).max(0.0);
        let top = b.min_y() + ds.px(crate::view::page::PAD) - self.scroll;
        self.view_h = b.size.height;
        let shown: Vec<Listed> = self.shown().into_iter().cloned().collect();
        let count = self.rows.as_ref().map_or(0, Vec::len);
        let mut page = Page {
            scene,
            text,
            icons,
            ds: &ds,
            x,
            width,
            y: top,
            clip: b,
            mouse: None,
            hits: Vec::new(),
            hover: None,
            timeline: None,
            flame: None,
            rows: Vec::new(),
            lit_row: self.lit,
            max_rows: MOST,
        };
        page.title(
            &t("profiler.recordings"),
            &tr!("profiler.recordings_count", count = count.to_string().as_str()),
        );
        // The filter.
        let field = Rect::new(x, page.y, width.min(ds.px(420.0)), ds.px(FIELD_H));
        let layer = page.scene.push_layer(Some(b));
        let typing = self.typing() && focused;
        self.filter.paint(layer, page.text, icons, &ds, (field, typing), &t("profiler.filter"));
        page.y += ds.px(FIELD_H + 16.0);
        if self.rows.is_none() {
            let reading = t("profiler.reading");
            let loader = ds::Loader::with_text(&reading);
            let layer = page.scene.push_layer(Some(b));
            ds::loader(layer, page.text, &ds, x, page.y + ds.px(12.0), &loader);
        } else if count == 0 {
            let layer = page.scene.push_layer(Some(b));
            let area = Rect::new(x, page.y, width, ds.px(120.0));
            ds::empty_state(
                layer,
                page.text,
                &ds,
                area,
                &t("profiler.no_recordings"),
                Some(&t("profiler.no_recordings_hint")),
            );
        } else {
            let rows: Vec<Vec<Cell>> = shown
                .iter()
                .map(|r| {
                    vec![
                        Cell::Text(r.when.clone()),
                        Cell::Muted(r.name.clone()),
                        Cell::Text(i18n::bytes(r.size)),
                    ]
                })
                .collect();
            let (c_when, c_file, c_size) =
                (t("profiler.col_when"), t("profiler.col_file"), t("profiler.col_size"));
            let (s_when, s_file, s_size) = (
                t("profiler.col_when_sub"),
                t("profiler.col_file_sub"),
                t("profiler.col_size_sub"),
            );
            page.table(
                &[
                    Col { name: &c_when, sub: &s_when, share: 0.3, right: false },
                    Col { name: &c_file, sub: &s_file, share: 0.5, right: false },
                    Col { name: &c_size, sub: &s_size, share: 0.2, right: true },
                ],
                &rows,
            );
        }
        self.content_h = page.y - top + ds.px(crate::view::page::PAD);
        self.drawn = std::mem::take(&mut page.rows);
        self.bar.paint(scene, palette, scale, b, self.content_h, self.scroll);
    }

    fn poll(&mut self) -> bool {
        self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.asked.is_some() || self.copying.is_some()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // On its parts: the scrollbar's thumb taken (or the page jumped there, a drag
        // follows), the filter's caret put or the filter cleared (it has the keys then);
        // elsewhere none has them.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            // × emptied the filter: the list from its top.
            if self.filter.take_cleared() {
                self.scroll = 0.0;
            }
            return None;
        }
        let path = self.path_of(self.row_at(p)?)?;
        Some(ViewerAction::OpenFile { path, line: None, column: None })
    }

    fn hover(&mut self, p: Point) -> bool {
        let lit = self.row_at(p);
        let changed = lit != self.lit;
        self.lit = lit;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    /// A part's (the filter's text cursor), else a hand on a row.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.controls.cursor_at(p).or_else(|| self.row_at(p).map(|_| ViewerCursor::Pointer))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, (self.content_h - self.view_h).max(0.0));
        self.scroll != before
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let row = self.row_at(p)?;
        self.menu_on = Some(self.path_of(row)?);
        Some(vec![
            ViewMenuEntry::item("open", t("profiler.open")).with_icon("file-text"),
            ViewMenuEntry::item("summary", t("profiler.copy_summary")).with_icon("copy"),
            ViewMenuEntry::item("reveal", t("profiler.reveal")).with_icon("folder"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item("delete", t("profiler.delete")).with_icon("trash"),
        ])
    }

    fn command(&mut self, id: &str) -> bool {
        // The filter with the keys: the app's editing commands are its (⌘A, ⌘X, ⌘Z, ⌘⇧Z).
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            if id != SELECT_ALL {
                self.scroll = 0.0;
            }
            return true;
        }
        if !matches!(id, "open" | "summary" | "reveal" | "delete") {
            return false;
        }
        let Some(path) = self.menu_on.take() else { return false };
        match id {
            "open" => {
                self.action = Some(ViewerAction::OpenFile { path, line: None, column: None });
            }
            "summary" => self.copy_summary(path),
            "reveal" => self.action = Some(ViewerAction::Request(AppRequest::Reveal(path))),
            _ => self.delete(path),
        }
        true
    }

    fn wants_text(&self) -> bool {
        self.typing()
    }

    /// ⌘C: what's selected in the filter while it has the keys.
    fn selected_text(&self) -> Option<String> {
        let filter: &dyn Control = &self.filter;
        self.controls.selected_text_with(|i| (i == FILTER).then_some(filter))
    }

    /// Pasted into the filter while it has the keys, as typed.
    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    /// Typed into the part with the keys (the filter): the list back at its top.
    fn text_input(&mut self, typed: &str) -> bool {
        let taken = self.with_controls(|c, parts| c.text(parts, typed)).is_some();
        if taken {
            self.scroll = 0.0;
        }
        taken
    }

    /// To the part with the keys; Esc on an empty filter gives them up.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let leaves = key == ViewerKey::Escape && self.filter.text().is_empty();
        self.with_controls(|c, parts| {
            let used = c.key(parts, key, reach).is_some();
            if used && leaves {
                c.focus(parts, None);
            }
            used
        })
    }

    /// Its scrollbar's thumb dragged: the page follows.
    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, changed)| changed)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

/// The filter as a control: the app's search field (a press puts its caret, a drag selects,
/// × empties it; the editing keys, ⌘A / ⌘X / ⌘Z, the copy of what's selected), the text
/// typed going in on one line (the field's own way: its breaks as spaces); Esc empties it. Tab is kept
/// (it does nothing here, the focus stays).
struct FilterField<'a> {
    field: &'a mut SearchField,
}

impl Control for FilterField<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.field)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        Control::press(&mut *self.field, p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut *self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(&mut *self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Escape {
            self.field.set_text("");
            return true;
        }
        Control::key(&mut *self.field, key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        Control::text(&mut *self.field, typed)
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field)
    }

    fn command(&mut self, id: &str) -> bool {
        Control::command(&mut *self.field, id)
    }
}

#[cfg(test)]
#[path = "../tests/unit/list.rs"]
mod tests;
