//! What every host of panes does the same way (a query's result, a table's rows): a button
//! in its bar per pane offered ([`PaneHost::paint_buttons`]: none offered, none shown), the
//! pane open under its grid in a split the user drags, the pointer, the wheel, the keys
//! (while the pane's list is open) and the tooltips routed to it, the rows lent anew only
//! when their version changes, and the pane's state kept with what's shown — read and
//! written on the host's pool, never on a click. What differs (how rows are lent, how all of
//! them are fetched) stays the host's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use kernel::{Point, Rect};
use render_api::Layer;
use text_layout::TextSystem;

use super::{Held, Lent, PaneRequest, ResultPane, ResultPaneProvider};
use crate::control::Press;
use crate::ds::{self, ButtonKind, ButtonSpec, Ctx};
use crate::{KeyReach, Tip, ViewerCursor, ViewerKey};

/// The pane's share of the room under the host's bar, at first.
const SPLIT: f32 = 0.5;
/// The least either side keeps (logical pixels).
const LEAST: f32 = 80.0;
/// The divider's grip, across (logical pixels).
const GRIP: f32 = 6.0;
/// The share a drag may give either side, at most.
const SHARE: (f32, f32) = (0.05, 0.95);

/// Runs a job on the host's pool.
pub type Spawn<'a> = &'a dyn Fn(Box<dyn FnOnce() + Send>);

/// The pane open now.
struct Open {
    id: &'static str,
    pane: Box<dyn ResultPane>,
    /// The rows' version last lent.
    lent: Option<u64>,
    /// Its state as last written.
    written: Option<String>,
}

/// A host's panes (see the module's doc).
#[derive(Default)]
pub struct PaneHost {
    offered: Vec<Arc<dyn ResultPaneProvider>>,
    open: Option<Open>,
    split: Option<f32>,
    divider: Rect,
    area: Rect,
    dragging: bool,
    held: bool,
    /// Where the mouse was last (the wheel goes to the pane only over it).
    mouse: Option<Point>,
    /// The states read for what's shown, by pane id, as they come.
    reading: Option<Receiver<Vec<(String, String)>>>,
    states: Vec<(String, String)>,
    /// Where they're kept, and under which key (`None`: nothing kept).
    kept: Option<(PathBuf, String)>,
    /// The states waiting to be written, the newest by file; one writer drains them.
    writes: Arc<std::sync::Mutex<Writes>>,
    /// The selection the open pane is drawn from instead of the rows (its version: a new
    /// one with each pick), until it's drawn from all of them again.
    picked: Option<(Lent, u64)>,
    picks: u64,
}

/// The right-click rows' ids: a pane drawn from the selection, from all the rows again,
/// closed. The host's own: a view hands them back to [`PaneHost::menu_command`].
pub const PANE_MENU: &str = "pane:";

/// States to write, the newest for each file, and whether a writer is on its way.
#[derive(Debug, Default)]
struct Writes {
    waiting: Vec<(PathBuf, PathBuf, String)>,
    writing: bool,
}

impl std::fmt::Debug for PaneHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneHost")
            .field("offered", &self.offered.len())
            .field("open", &self.open.as_ref().map(|o| o.id))
            .finish_non_exhaustive()
    }
}

impl PaneHost {
    /// The panes offered now; their states kept in `folder` under `shown`'s key (a query's
    /// text, a table's name), read in the background with `spawn`.
    pub fn new(folder: Option<PathBuf>, shown: &str, spawn: Spawn<'_>) -> Self {
        let offered = super::providers();
        let mut host = Self { offered, ..Self::default() };
        let Some(folder) = folder else { return host };
        let key = key_of(shown);
        if !host.offered.is_empty() {
            let (tx, rx) = std::sync::mpsc::channel();
            let ids: Vec<&'static str> = host.offered.iter().map(|p| p.id()).collect();
            let (dir, k) = (folder.clone(), key.clone());
            spawn(Box::new(move || {
                let read = ids
                    .into_iter()
                    .filter_map(|id| {
                        let text = std::fs::read_to_string(dir.join(file_name(id, &k))).ok()?;
                        Some((id.to_owned(), text))
                    })
                    .collect();
                let _ = tx.send(read);
            }));
            host.reading = Some(rx);
        }
        host.kept = Some((folder, key));
        host
    }

    pub fn offered(&self) -> &[Arc<dyn ResultPaneProvider>] {
        &self.offered
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.open.as_ref().is_some_and(|o| o.id == id)
    }

    /// Pane `i` (of the offered) opened, or closed when it's the one open.
    /// Either way it's drawn from all the rows after (a selection it was drawn from let go).
    pub fn toggle(&mut self, i: usize) {
        let Some(provider) = self.offered.get(i) else { return };
        self.picked = None;
        if self.is_open(provider.id()) {
            if let Some(mut open) = self.open.take() {
                open.pane.hidden();
            }
            return;
        }
        let mut pane = provider.make();
        if let Some((_, state)) = self.states.iter().find(|(id, _)| id == provider.id()) {
            pane.restore(state);
        }
        let written = pane.saved();
        self.open = Some(Open { id: provider.id(), pane, lent: None, written });
    }

    /// Its buttons from `right` leftwards in a bar (`mid`: its middle, `side`: a button's),
    /// the open one on; each hit with its place among the offered. Returns where they begin.
    pub fn paint_buttons(
        &self,
        (layer, text, icons, ctx): (&mut Layer, &mut TextSystem, &icons::Icons, &Ctx<'_>),
        (right, mid, side): (f32, f32, f32),
        hovered: Option<usize>,
        hits: &mut Vec<(Rect, usize)>,
    ) -> f32 {
        let mut right = right;
        for (i, provider) in self.offered.iter().enumerate().rev() {
            let r = Rect::new(right - side, (mid - side / 2.0).round(), side, side);
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon(provider.icon())
                .on(self.is_open(provider.id()))
                .hovered(hovered == Some(i));
            ds::button(layer, text, icons, ctx, r, spec);
            hits.push((r, i));
            right = r.min_x() - ctx.px(2.0);
        }
        right
    }

    /// A button's tooltip (its pane's title).
    pub fn button_tip(&self, i: usize, at: Rect) -> Option<Tip> {
        Some(Tip::new(at, self.offered.get(i)?.title()))
    }

    /// Where the grid and the pane go in `area` (the pane: `None` while none is open).
    pub fn split(&mut self, area: Rect, scale: f32) -> (Rect, Option<Rect>) {
        if self.open.is_none() {
            self.area = Rect::default();
            self.divider = Rect::default();
            return (area, None);
        }
        let least = (LEAST * scale).min(area.size.height / 2.0);
        let share = self.split.unwrap_or(SPLIT);
        let pane_h = (area.size.height * share).clamp(least, (area.size.height - least).max(least));
        let grid =
            Rect::new(area.min_x(), area.min_y(), area.size.width, area.size.height - pane_h);
        let pane = Rect::new(area.min_x(), grid.max_y(), area.size.width, pane_h);
        let grip = GRIP * scale;
        self.divider = Rect::new(area.min_x(), pane.min_y() - grip / 2.0, area.size.width, grip);
        self.area = pane;
        (grid, Some(pane))
    }

    pub fn pane(&mut self) -> Option<&mut dyn ResultPane> {
        self.open.as_mut().map(|o| &mut *o.pane as &mut dyn ResultPane)
    }

    pub fn pane_ref(&self) -> Option<&dyn ResultPane> {
        self.open.as_ref().map(|o| &*o.pane as &dyn ResultPane)
    }

    /// `p` is on the pane or its divider.
    pub fn over(&self, p: Point) -> bool {
        self.open.is_some() && (self.area.contains(p) || self.divider.contains(p))
    }

    pub fn on_divider(&self, p: Point) -> bool {
        self.open.is_some() && self.divider.contains(p)
    }

    /// The pane takes the keys and the text now (its list open).
    pub fn captures(&self) -> bool {
        self.pane_ref().is_some_and(|p| p.captures_keys())
    }

    /// A press over the pane: its divider taken, or the pane's own (`true`: taken).
    pub fn press(&mut self, p: Point, clicks: u32) -> bool {
        if self.on_divider(p) {
            self.dragging = true;
            return true;
        }
        let inside = self.area.contains(p);
        let Some(open) = &mut self.open else { return false };
        if !inside {
            return false;
        }
        match open.pane.press(p, clicks) {
            Press::Held => {
                self.held = true;
                true
            }
            Press::Took => true,
            Press::Ignored => false,
        }
    }

    /// The pointer moved while the divider or the pane holds it (`None`: neither does);
    /// `whole`: the room the split is made in.
    pub fn drag(&mut self, p: Point, whole: Rect) -> Option<bool> {
        if self.dragging {
            let h = whole.size.height.max(1.0);
            self.split = Some(((whole.max_y() - p.y) / h).clamp(SHARE.0, SHARE.1));
            return Some(true);
        }
        if self.held {
            return self.open.as_mut().map(|o| o.pane.drag(p));
        }
        None
    }

    /// The pointer let go (`None`: nothing of the panes held it).
    pub fn release(&mut self) -> Option<bool> {
        if std::mem::take(&mut self.dragging) {
            return Some(true);
        }
        if std::mem::take(&mut self.held) {
            return self.open.as_mut().map(|o| o.pane.release());
        }
        None
    }

    /// The mouse over the host at `p`: the pane told whether it's over it.
    pub fn hover(&mut self, p: Point) -> bool {
        self.mouse = Some(p);
        let inside = self.area.contains(p);
        self.open.as_mut().is_some_and(|o| o.pane.hover(inside.then_some(p)))
    }

    /// The wheel, when the mouse is over the pane: the pane's (it zooms, its list scrolls),
    /// and never on to the grid under it, even when the pane had nothing to scroll.
    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if !self.mouse.is_some_and(|p| self.area.contains(p) || self.divider.contains(p)) {
            return false;
        }
        if let Some(pane) = self.pane() {
            pane.scroll(dx, dy);
        }
        true
    }

    /// A key, while the pane takes them.
    pub fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.captures() && self.pane().is_some_and(|pane| pane.key(key, reach))
    }

    /// Text typed, while the pane takes it.
    pub fn text(&mut self, typed: &str) -> bool {
        self.captures() && self.pane().is_some_and(|pane| pane.text(typed))
    }

    pub fn tooltip(&self, p: Point) -> Option<Tip> {
        self.over(p).then(|| self.pane_ref()?.tooltip(p)).flatten()
    }

    pub fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        if !self.over(p) {
            return None;
        }
        if self.on_divider(p) {
            return Some(ViewerCursor::ResizeVertical);
        }
        self.pane_ref()?.cursor(p)
    }

    /// The open pane lent `rows` as of `version` (nothing when it's the version it has).
    /// While it's drawn from the selection, that's lent instead (the rows' versions don't
    /// matter then).
    pub fn lend(&mut self, version: u64, rows: impl FnOnce() -> Lent, held: Held) -> bool {
        let Some(open) = &mut self.open else { return false };
        if let Some((picked, pick)) = &self.picked {
            // Versions of picks kept apart from the rows' (the top bit set).
            let version = pick | 1 << 63;
            if open.lent == Some(version) {
                return false;
            }
            open.lent = Some(version);
            let held = Held { rows: picked.rows(), picked: true, ..Held::default() };
            open.pane.set_rows(Arc::clone(picked), version, held);
            return true;
        }
        if open.lent == Some(version) {
            return false;
        }
        open.lent = Some(version);
        open.pane.set_rows(rows(), version, held);
        true
    }

    /// Pane `id` (opened if it's closed) drawn from `picked`, the grid's selection, until it's
    /// drawn from all the rows again ([`Self::whole_rows`]).
    pub fn pick(&mut self, id: &str, picked: Lent) {
        if !self.is_open(id) {
            let Some(i) = self.offered.iter().position(|p| p.id() == id) else { return };
            self.toggle(i);
        }
        self.picks = self.picks.wrapping_add(1);
        self.picked = Some((picked, self.picks));
        self.lend_again();
    }

    /// The open pane drawn from all the rows again (not the selection).
    pub fn whole_rows(&mut self) {
        if self.picked.take().is_some() {
            self.lend_again();
        }
    }

    /// Drawn from the grid's selection now.
    pub fn is_picked(&self) -> bool {
        self.picked.is_some()
    }

    /// The rows a right-click on the grid adds (`selected`: two cells or more are): each
    /// pane offered drawn from the selection; the open one from all the rows again (while
    /// it's drawn from a selection), or closed. None while nothing is offered.
    pub fn menu(&self, selected: bool) -> Vec<crate::ViewMenuEntry> {
        use crate::ViewMenuEntry;
        let mut rows = Vec::new();
        for p in &self.offered {
            rows.push(
                ViewMenuEntry::item(
                    format!("{PANE_MENU}pick:{}", p.id()),
                    i18n::tr!("result_pane.from_selection", pane = p.title()),
                )
                .with_icon(p.icon())
                .disabled_if(!selected),
            );
        }
        if let Some(open) = &self.open {
            let title = self.offered.iter().find(|p| p.id() == open.id).map(|p| p.title());
            let title = title.unwrap_or_default();
            if self.picked.is_some() {
                rows.push(ViewMenuEntry::item(
                    format!("{PANE_MENU}whole"),
                    i18n::tr!("result_pane.whole_rows", pane = title.as_str()),
                ));
            }
            rows.push(
                ViewMenuEntry::item(
                    format!("{PANE_MENU}close"),
                    i18n::tr!("result_pane.close", pane = title.as_str()),
                )
                .with_icon("x"),
            );
        }
        if !rows.is_empty() {
            rows.insert(0, ViewMenuEntry::Separator);
        }
        rows
    }

    /// A row of [`Self::menu`] chosen (`picked` asked only to draw from the selection):
    /// `true` if it was one.
    pub fn menu_command(&mut self, id: &str, picked: impl FnOnce() -> Option<Lent>) -> bool {
        let Some(asked) = id.strip_prefix(PANE_MENU) else { return false };
        match asked {
            "whole" => self.whole_rows(),
            "close" => {
                self.picked = None;
                if let Some(mut open) = self.open.take() {
                    open.pane.hidden();
                }
            }
            _ => {
                let Some(pane) = asked.strip_prefix("pick:") else { return false };
                if let Some(rows) = picked() {
                    self.pick(pane, rows);
                }
            }
        }
        true
    }

    /// The rows lent again at the next [`Self::lend`] (how many the host holds changed).
    pub fn lend_again(&mut self) {
        if let Some(open) = &mut self.open {
            open.lent = None;
        }
    }

    /// The states read meanwhile, the pane's own work, its state written once it changed;
    /// `true` to draw again, and what it asks of the host.
    pub fn poll(&mut self, spawn: Spawn<'_>) -> (bool, Option<PaneRequest>) {
        if let Some(rx) = &self.reading
            && let Ok(states) = rx.try_recv()
        {
            self.states = states;
            self.reading = None;
        }
        let Some(open) = &mut self.open else { return (false, None) };
        let mut changed = open.pane.poll();
        let mut request = open.pane.take_request();
        // Drawn from all the rows again: the host's own (lent at the next lend).
        if request == Some(PaneRequest::WholeRows) {
            request = None;
            if self.picked.take().is_some() {
                open.lent = None;
                changed = true;
            }
        }
        let saved = open.pane.saved();
        if saved != open.written {
            if let (Some((folder, key)), Some(text)) = (&self.kept, &saved) {
                let (dir, path) = (folder.clone(), folder.join(file_name(open.id, key)));
                queue_write(&self.writes, (dir, path, text.clone()), spawn);
            }
            let text = saved.clone().unwrap_or_default();
            match self.states.iter_mut().find(|(id, _)| id == open.id) {
                Some(kept) => kept.1 = text,
                None => self.states.push((open.id.to_owned(), text)),
            }
            open.written = saved;
        }
        (changed, request)
    }

    /// A command for the open pane (the path its "Save As" chose): `true` if it took it.
    pub fn command(&mut self, id: &str) -> bool {
        self.pane().is_some_and(|pane| pane.command(id))
    }

    /// The pane moves (a transition, something lighting up): a frame is wanted.
    pub fn animating(&self) -> bool {
        self.pane_ref().is_some_and(|p| p.animating())
    }

    /// The pane waits for work of its own.
    pub fn busy(&self) -> bool {
        self.pane_ref().is_some_and(|p| p.busy())
    }
}

/// A state to write: it replaces one still waiting for the same file; a writer is started
/// only when none is on its way. Writes go one at a time, each the newest state then.
fn queue_write(
    writes: &Arc<std::sync::Mutex<Writes>>,
    write_one: (PathBuf, PathBuf, String),
    spawn: Spawn<'_>,
) {
    let Ok(mut w) = writes.lock() else { return };
    match w.waiting.iter_mut().find(|(_, path, _)| *path == write_one.1) {
        Some(waiting) => *waiting = write_one,
        None => w.waiting.push(write_one),
    }
    if w.writing {
        return;
    }
    w.writing = true;
    drop(w);
    let writes = Arc::clone(writes);
    spawn(Box::new(move || drain_writes(&writes)));
}

/// Writes what waits, the newest taken under the lock and written after it, until none is
/// left (then the next state starts a writer again).
fn drain_writes(writes: &std::sync::Mutex<Writes>) {
    loop {
        let next = {
            let Ok(mut w) = writes.lock() else { return };
            if w.waiting.is_empty() {
                w.writing = false;
                return;
            }
            w.waiting.remove(0)
        };
        let (dir, path, text) = next;
        if let Err(e) = write(&dir, &path, &text) {
            tracing::warn!(error = %e, "can't keep a result pane's state");
        }
    }
}

/// The temporary files' numbers: each write's own (two never share one).
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `text` written to `path` whole or not at all (a temporary file of its own, renamed over
/// it).
fn write(dir: &Path, path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let n = TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

/// The key a shown thing's states are kept under: a stable fingerprint of its text (FNV-1a,
/// the same in every build), its blanks at the ends left out.
pub fn key_of(shown: &str) -> String {
    format!("{:016x}", kernel::hash::fnv64(shown.trim()))
}

/// A pane's state file for a key.
fn file_name(id: &str, key: &str) -> String {
    format!("{key}-{id}.txt")
}

#[cfg(test)]
#[path = "../../tests/unit/result_pane_host.rs"]
mod tests;
