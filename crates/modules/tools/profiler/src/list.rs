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
use ui_components::ds::{self, Ctx};
use ui_components::{
    AppRequest, EdgeScrollbar, KeyReach, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor,
    ViewerKey, ViewerPaint, WithHost,
};
use ui_kit::TextInput;

use crate::view::page::{Cell, Col, Page};
use crate::view::words;

/// The extension the tab is opened by.
pub const EXTENSION: &str = "view-profiler-recordings";
/// The recordings listed at most (the newest).
const MOST: usize = 500;
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
    filter: TextInput,
    typing: bool,
    field: Rect,
    clear: Option<Rect>,
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
            filter: TextInput::default(),
            typing: false,
            field: Rect::default(),
            clear: None,
            drawn: Vec::new(),
            lit: None,
            menu_on: None,
            scroll: 0.0,
            content_h: 0.0,
            view_h: 0.0,
            bar: EdgeScrollbar::default(),
            waker: None,
            action: None,
        }
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
        self.field = Rect::new(x, page.y, width.min(ds.px(420.0)), ds.px(FIELD_H));
        {
            let layer = page.scene.push_layer(Some(b));
            let mut pen = ui_components::LayerPainter::new(layer, page.text, icons, ds);
            ds::search_input(
                &mut pen,
                self.field,
                &self.filter,
                self.typing && focused,
                &t("profiler.filter"),
            );
            self.clear = None;
            if !self.filter.text().is_empty() {
                let mid = self.field.min_y() + self.field.size.height / 2.0;
                let close = ds::clear_button(
                    pen.layer,
                    icons,
                    &ds,
                    self.field.max_x() - ds.px(18.0),
                    mid,
                    false,
                );
                self.clear = Some(close);
            }
        }
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
        // On its scrollbar: the thumb taken (or the page jumped there); a drag follows.
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        if self.clear.is_some_and(|r| r.contains(p)) {
            self.filter = TextInput::default();
            self.typing = true;
            return None;
        }
        self.typing = self.field.contains(p);
        if self.typing {
            self.filter.press(p.x, 1);
            return None;
        }
        let path = self.path_of(self.row_at(p)?)?;
        Some(ViewerAction::OpenFile { path, line: None, column: None })
    }

    fn hover(&mut self, p: Point) -> bool {
        let lit = self.row_at(p);
        let changed = lit != self.lit;
        self.lit = lit;
        self.bar.hover(p) | changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.field.contains(p) {
            return Some(ViewerCursor::Text);
        }
        self.row_at(p).map(|_| ViewerCursor::Pointer)
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
        self.typing
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if !self.typing {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.filter.insert(&typed);
        self.scroll = 0.0;
        true
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !self.typing {
            return false;
        }
        if key == ViewerKey::Escape {
            if self.filter.text().is_empty() {
                self.typing = false;
            } else {
                self.filter = TextInput::default();
            }
            return true;
        }
        reach.edit(&mut self.filter, key)
    }

    /// Its scrollbar's thumb dragged: the page follows.
    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        changed
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/list.rs"]
mod tests;
