//! A file's local history, in a tab: its versions newest first (age, date, why it was kept,
//! size; the one matching the file now marked). A click compares a version with the file;
//! "Revert" brings it back into the editor (unsaved, one undo step). The window's fixed tab
//! ("Current file", [`FollowData`]) shows the active file's, following the editor's tabs while
//! it's seen ([`crate::follow`]). The versions are read on a worker, never while drawing.

use std::path::PathBuf;
use std::sync::Arc;

use history::{HistoryStore, Revision, RevisionKind, text_hash};
use history_fs::DiskHistory;
use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};
use ui_kit::draw_text;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-local-history";

/// What the fixed tab is opened with: the project (its active file followed), its root and
/// its versions' store (`None`: no project open — the tab says so).
#[derive(Debug, Clone)]
pub struct FollowData {
    pub project: Option<kernel::ProjectId>,
    pub root: PathBuf,
    pub store: Option<Arc<DiskHistory>>,
}

/// What a read of the versions brought: the list, and the file's hash now.
type Read = (Vec<Revision>, Option<u64>);

/// What the fixed tab shows when it shows no versions' list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// No file is active (or no project is open).
    NoFile,
    /// The active file isn't the project's.
    Outside,
}

/// What the view is opened with.
#[derive(Debug, Clone)]
pub struct HistoryData {
    pub store: Arc<DiskHistory>,
    /// The file, relative to the project (the store's key), and on disk.
    pub path: String,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Row(usize),
    Revert(usize),
}

#[derive(Debug, Default)]
pub struct HistoryView {
    data: Option<HistoryData>,
    revisions: Vec<Revision>,
    /// The hash of the file as it is on disk (the version matching it is marked).
    current: Option<u64>,
    selected: Option<usize>,
    scroll: f32,
    list: Rect,
    row_h: f32,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    pending: Option<ViewerAction>,
    /// A comparison being read.
    opening: ui_components::deferred::Deferred,
    /// The versions' scrollbar, hidden at rest (beside them, in the margin).
    bar: ui_components::EdgeScrollbar,
    /// Its controls' state of use (see [`HistoryView::with_controls`]).
    controls: Controls,
    /// The fixed tab: what it follows, and the change of the active file it last took.
    follow: Option<FollowData>,
    followed: Option<u64>,
    /// Why it shows no list (the fixed tab without a project's file).
    shown: Option<Shown>,
    /// A read of the versions going on (on a worker), and what wakes the app when it's done.
    reading: Option<std::sync::mpsc::Receiver<Read>>,
    waker: Option<Wake>,
}

/// What wakes the app (a read done).
#[derive(Clone)]
struct Wake(Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wake")
    }
}

impl HistoryView {
    /// Its controls (the versions' scrollbar with its offset), the topmost first, with
    /// their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut bar = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 1] = [&mut bar];
        f(&mut self.controls, &mut parts)
    }

    /// The versions listed (tests).
    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    /// Reads the list again (names only) and the file's current hash, on a worker; what it
    /// brings is taken at the next poll.
    fn reload(&mut self) {
        let Some(data) = self.data.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("local-history", move || {
            let revisions = data.store.revisions(&data.path);
            let current = std::fs::read(&data.file).ok().map(|t| {
                crate::DISK.read(t.len() as u64);
                text_hash(&t)
            });
            let _ = tx.send((revisions, current));
            if let Some(Wake(wake)) = waker {
                wake();
            }
        });
        self.reading = Some(rx);
    }

    /// A read come in: `true` if it did.
    fn take_read(&mut self) -> bool {
        let Some(rx) = &self.reading else { return false };
        let Ok((revisions, current)) = rx.try_recv() else { return false };
        self.revisions = revisions;
        self.current = current;
        self.reading = None;
        self.selected = self.selected.filter(|i| *i < self.revisions.len());
        true
    }

    /// The fixed tab: the active file taken, when it changed since (its versions read).
    fn follow_active(&mut self) -> bool {
        let Some(follow) = &self.follow else { return false };
        let Some(project) = follow.project else {
            let changed = self.shown != Some(Shown::NoFile);
            self.shown = Some(Shown::NoFile);
            return changed;
        };
        crate::follow::seen(project);
        let (generation, file) = crate::follow::file(project);
        // Not asked yet (the first poll comes after it's drawn): nothing said meanwhile.
        if generation == 0 || self.followed == Some(generation) {
            return false;
        }
        self.followed = Some(generation);
        let (root, store) = (follow.root.clone(), follow.store.clone());
        let relative = file.as_deref().and_then(|f| version_control::relative_path(&root, f));
        match (file, relative, store) {
            (Some(file), Some(path), Some(store)) => {
                if self.data.as_ref().is_some_and(|d| d.file == file) {
                    return false;
                }
                self.shown = None;
                self.take(HistoryData { store, path, file });
            }
            (Some(_), None, _) => self.clear(Shown::Outside),
            _ => self.clear(Shown::NoFile),
        }
        true
    }

    /// `data`'s versions shown (read on a worker).
    fn take(&mut self, data: HistoryData) {
        self.data = Some(data);
        self.revisions.clear();
        self.current = None;
        self.selected = None;
        self.scroll = 0.0;
        self.reload();
    }

    /// No list: why (the fixed tab).
    fn clear(&mut self, shown: Shown) {
        self.data = None;
        self.revisions.clear();
        self.reading = None;
        self.selected = None;
        self.shown = Some(shown);
    }

    /// Version `i` compared with the file.
    fn compare(&mut self, i: usize) -> Option<ViewerAction> {
        let data = self.data.clone()?;
        // Nobody shows comparisons: the file itself, rather than a click that does nothing.
        if !workbench_model::presence::compare() {
            return Some(ViewerAction::OpenFile { path: data.file, line: None, column: None });
        }
        let at = self.revisions.get(i)?.at;
        // Read in the background: the version and the file may be large.
        self.opening.start("local-history", move || {
            let current = std::fs::read_to_string(&data.file).unwrap_or_default();
            crate::DISK.read(current.len() as u64);
            let label = when(at);
            let input = history::application::compare_with_current(
                data.store.as_ref(),
                &data.path,
                at,
                &data.file,
                &current,
                &label,
            )?;
            let name = data.path.rsplit('/').next().unwrap_or(&data.path);
            Some(ViewerAction::CompareBeside(Box::new(input), format!("{name} · {label}")))
        });
        None
    }

    /// Version `i` back in the editor.
    fn revert(&self, i: usize) -> Option<ViewerAction> {
        let data = self.data.as_ref()?;
        let revision = self.revisions.get(i)?;
        let text = data.store.read(&data.path, revision.at)?;
        Some(ViewerAction::SetText {
            path: data.file.clone(),
            text: String::from_utf8_lossy(&text).into_owned(),
        })
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn reveal(&mut self, i: usize) {
        let top = i as f32 * self.row_h;
        if top < self.scroll {
            self.scroll = top;
        } else if top + self.row_h > self.scroll + self.list.size.height {
            self.scroll = top + self.row_h - self.list.size.height;
        }
    }
}

/// `2026-10-21 14:32` (UTC).
fn when(at: u64) -> String {
    let seconds = (at / 1000) as i64;
    let (y, m, d) = version_control::date(seconds);
    let in_day = seconds.rem_euclid(86_400);
    format!("{y}-{m:02}-{d:02} {:02}:{:02}", in_day / 3600, in_day % 3600 / 60)
}

/// `12 KB`.
fn size(bytes: u64) -> String {
    if bytes < 1024 { format!("{bytes} B") } else { format!("{} KB", bytes.div_ceil(1024)) }
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Viewer for HistoryView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(follow) = data.downcast_ref::<FollowData>() {
            self.follow = Some(follow.clone());
            self.followed = None;
            self.follow_active();
            return;
        }
        let Some(data) = data.downcast_ref::<HistoryData>() else { return };
        self.take(data.clone());
    }

    /// Its action: `reload` (the module's command).
    fn command(&mut self, id: &str) -> bool {
        if id == "reload" {
            self.reload();
            return true;
        }
        false
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        let last = self.revisions.len().saturating_sub(1);
        let next = match (key, self.selected) {
            (ViewerKey::Down, Some(i)) => (i + 1).min(last),
            (ViewerKey::Up, Some(i)) => i.saturating_sub(1),
            (ViewerKey::Down | ViewerKey::Up, None) => 0,
            (ViewerKey::Enter, Some(i)) => {
                self.pending = self.compare(i);
                return true;
            }
            _ => return false,
        };
        if self.revisions.is_empty() {
            return false;
        }
        self.selected = Some(next);
        self.reveal(next);
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // Its controls first: the scrollbar's thumb is held or jumped to.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        match self.hit(p)? {
            Hit::Row(i) => {
                self.selected = Some(i);
                self.compare(i)
            }
            Hit::Revert(i) => self.revert(i),
        }
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        let content = self.revisions.len() as f32 * self.row_h;
        self.scroll = (self.scroll + dy).clamp(0.0, (content - self.list.size.height).max(0.0));
        self.scroll != before
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let rows = || self.hit(p).map(|_| ViewerCursor::Pointer);
        self.controls.cursor_at(p).or_else(rows)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    /// Its scrollbar fading: a frame when it next changes (as its parts said at the paint).
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn status(&self) -> Option<String> {
        Some(tr!("history.versions", count = self.revisions.len()))
    }

    fn poll(&mut self) -> bool {
        let opened = self.opening.poll();
        let ready = opened.is_some();
        if ready {
            self.pending = opened;
        }
        let read = self.take_read();
        ready || read || self.opening.is_busy()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.paint_page(ctx);
        // What its parts want of the frames, as drawn.
        self.with_controls(|c, parts| c.settle(parts));
    }
}

impl HistoryView {
    /// The page: the title, the explanation, the versions and their scrollbar.
    fn paint_page(&mut self, ctx: ViewerPaint<'_>) {
        self.opening.set_waker(&ctx.waker);
        if let Some(wake) = &ctx.waker {
            self.waker = Some(Wake(Arc::clone(wake)));
        }
        // Drawn: it shows — the fixed tab follows the active file (only while it does).
        self.follow_active();
        self.take_read();
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let width = px(860.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = b.min_y() + px(28.0);

        // The title, the file.
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let title = ctx.text.shape_line(&t("history.title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        if let Some(data) = &self.data {
            let bx = x0 + title.width + px(12.0);
            ds::badge(
                layer,
                ctx.text,
                &ds,
                ds::BadgeSpec {
                    label: &data.path,
                    x: bx,
                    mid: y + px(14.0),
                    align_right: false,
                    tone: Tone::Accent,
                },
            );
        }
        y += px(48.0);
        let small = ds.ui_font(-1.0);
        let summary = ctx.text.shape_line(&t("history.explain"), &small);
        draw_text(layer, &summary, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
        y += px(34.0);

        self.list = Rect::new(x0, y, width, (b.max_y() - y - px(16.0)).max(0.0));
        if self.revisions.is_empty() {
            let (title, detail) = match self.shown {
                Some(Shown::NoFile) => (t("history.no_file"), Some(t("history.no_file_detail"))),
                Some(Shown::Outside) => (t("history.outside_project"), None),
                // Still being read: nothing said yet.
                None if self.reading.is_some() || self.data.is_none() => return,
                None => (t("history.empty"), Some(t("history.empty_detail"))),
            };
            ds::empty_state(layer, ctx.text, &ds, self.list, &title, detail.as_deref());
            return;
        }
        let font = ds.ui_font(-0.5);
        self.row_h = px(40.0);
        let rows = ctx.scene.push_layer(Some(self.list));
        let now = now_seconds();
        let first = (self.scroll / self.row_h).floor() as usize;
        let visible = (self.list.size.height / self.row_h).ceil() as usize + 2;
        for i in first..(first + visible).min(self.revisions.len()) {
            let revision = &self.revisions[i];
            let top = (self.list.min_y() + i as f32 * self.row_h - self.scroll).round();
            let row = Rect::new(x0, top, width, self.row_h - px(4.0));
            let hovered = matches!(self.hover, Some(Hit::Row(h) | Hit::Revert(h)) if h == i);
            let selected = self.selected == Some(i);
            let fill = if selected {
                p.accent.with_alpha(0.14)
            } else if hovered {
                p.text.with_alpha(0.05)
            } else {
                p.text.with_alpha(0.02)
            };
            rows.quad(Quad::filled(row, fill).rounded(px(radius::MD)));
            // Only the visible part: a row under the header must not take its clicks.
            if let Some(hit) = row.intersect(&self.list) {
                self.hits.push((hit, Hit::Row(i)));
            }
            let mid = row.min_y() + row.size.height / 2.0;
            let base = ds::optical_baseline(row.min_y(), row.size.height, &font);
            // Age (prominent), then the date and why it was kept.
            let age = i18n::ago(now - (revision.at / 1000) as i64);
            let shaped = ctx.text.shape_line(&age, &font);
            let mut x = row.min_x() + px(14.0);
            draw_text(rows, &shaped, x, base, p.text);
            x += px(150.0).max(shaped.width + px(12.0));
            let date = ctx.text.shape_line(&when(revision.at), &small);
            let small_base = ds::optical_baseline(row.min_y(), row.size.height, &small);
            draw_text(rows, &date, x, small_base, p.text_muted);
            x += date.width + px(14.0);
            let kind = match revision.kind {
                RevisionKind::Saved => t("history.saved"),
                RevisionKind::BeforeSave => t("history.before_save"),
            };
            let chip = ds::badge(
                rows,
                ctx.text,
                &ds,
                ds::BadgeSpec { label: &kind, x, mid, align_right: false, tone: Tone::Neutral },
            );
            x = chip.max_x() + px(8.0);
            if self.current == Some(revision.hash) {
                ds::badge(
                    rows,
                    ctx.text,
                    &ds,
                    ds::BadgeSpec {
                        label: &t("history.current"),
                        x,
                        mid,
                        align_right: false,
                        tone: Tone::Accent,
                    },
                );
            }
            // Size, and "Revert" on the right (on hover / selection).
            let mut right = row.max_x() - px(10.0);
            if hovered || selected {
                let label = t("history.revert");
                let shaped = ctx.text.shape_line(&label, &small);
                let w = shaped.width + px(22.0);
                let r =
                    Rect::new((right - w).round(), (mid - px(13.0)).round(), w.round(), px(26.0));
                let on = self.hover == Some(Hit::Revert(i));
                let bg = if on { p.accent } else { p.accent.with_alpha(0.14) };
                rows.quad(Quad::filled(r, bg).rounded(px(radius::MD)));
                let color = if on { kernel::Rgba::WHITE } else { p.accent };
                draw_text(
                    rows,
                    &shaped,
                    (r.min_x() + px(11.0)).round(),
                    ds::optical_baseline(r.min_y(), r.size.height, &small),
                    color,
                );
                if let Some(hit) = r.intersect(&self.list) {
                    self.hits.push((hit, Hit::Revert(i)));
                }
                right = r.min_x() - px(12.0);
            }
            let shaped = ctx.text.shape_line(&size(revision.size), &small);
            draw_text(rows, &shaped, (right - shaped.width).round(), small_base, p.text_muted);
        }
        // The scrollbar just right of the rows (in the margin around them).
        let l = self.list;
        let lane = Rect::new(l.min_x(), l.min_y(), l.size.width + px(14.0), l.size.height);
        let content = self.revisions.len() as f32 * self.row_h;
        self.bar.set_auto_hiding(true);
        self.bar.paint(ctx.scene, p, ctx.scale, lane, content, self.scroll);
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
