//! Pull / Push / Sync, reviewed before anything moves (a tab): what changed on either side
//! since the last transfer — for the project, a folder or a file — each change with why
//! (new / changed / removed, here or on the server), checked to be done. Removals wait
//! unchecked; a file changed on both sides is a conflict: the server's copy or ours is
//! chosen, compared beside first. "Apply" transfers the checked ones in parallel (each in
//! the Remote log) — the list locked meanwhile, the bytes moved shown on a bar, and a stop
//! that ends it mid-file (nothing half-written is left in place) — then says how it went.
//!
//! Comparing with the server and transferring run on their own threads (`remote-plan`,
//! `remote-apply`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use remote::application::{
    Action, Change, Direction, LocalFiles, RemoteFs, Run, apply, in_scope, list_remote_under, plan,
};
use remote::{RemoteError, content_hash};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Check, Ctx, Tone, radius};
use ui_components::{
    Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;

use crate::paint::{Column, Pen};
use crate::session::{self, State};

/// The extension of the review's tab.
pub const EXTENSION: &str = "view-remote-review";

/// Conflicts whose two copies are read for comparing (the first ones), and their size limit.
const COMPARED: usize = 50;
const COMPARED_BYTES: u64 = 2 * 1024 * 1024;

/// What the review is opened with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewData {
    /// The remote project (its identity: its session goes by it).
    pub project: kernel::ProjectId,
    /// Its root (where its files are).
    pub root: PathBuf,
    pub direction: Direction,
    /// A folder or a file (relative, with `/`); `""`: the whole project.
    pub scope: String,
    /// Only the files brought already (an on-demand project's Pull / Sync): the server's
    /// others stay there.
    pub fetched_only: bool,
}

#[derive(Debug)]
pub(crate) struct ReviewProvider;

impl ViewerProvider for ReviewProvider {
    fn id(&self) -> &'static str {
        "remote.review"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(TransferReview::default()))
    }
}

/// A change, and both copies of a conflict (server, here) to compare.
#[derive(Debug, Clone)]
struct Row {
    change: Change,
    texts: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Planning,
    Review,
    Applying,
    /// `stopped`: the user stopped it (the rest wasn't transferred).
    Done {
        done: usize,
        failed: usize,
        stopped: bool,
    },
    Failed(String),
}

enum Message {
    Planned(Result<Vec<Row>, RemoteError>),
    Applied { done: usize, failed: usize, stopped: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Check(usize),
    /// Every change checked or unchecked (conflicts stay: a side is chosen for each).
    CheckAll,
    /// Every removal checked or unchecked.
    CheckRemovals,
    /// A conflict decided: the server's copy (`true`) or ours.
    Take(usize, bool),
    Compare(usize),
    Apply,
    Close,
    /// Stops the transfer going on.
    Stop,
}

pub struct TransferReview {
    data: Option<ReviewData>,
    started: bool,
    phase: Phase,
    rows: Vec<Row>,
    rx: Option<Receiver<Message>>,
    /// Transfers done / to do while applying.
    progress: Arc<(AtomicUsize, AtomicUsize)>,
    /// Bytes moved / to move while applying.
    bytes: Arc<(AtomicU64, AtomicU64)>,
    cancel: Arc<AtomicBool>,
    highlight: Option<usize>,
    scroll: f32,
    max_scroll: f32,
    bar: ui_components::EdgeScrollbar,
    /// The highlighted row last scrolled into sight.
    shown: Option<usize>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Default for TransferReview {
    fn default() -> Self {
        Self {
            data: None,
            started: false,
            phase: Phase::Planning,
            rows: Vec::new(),
            rx: None,
            progress: Arc::default(),
            bytes: Arc::default(),
            cancel: Arc::default(),
            highlight: None,
            scroll: 0.0,
            max_scroll: 0.0,
            bar: Default::default(),
            shown: None,
            hits: Vec::new(),
            hover: None,
            action: None,
            waker: None,
        }
    }
}

impl std::fmt::Debug for TransferReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransferReview").field("phase", &self.phase).finish_non_exhaustive()
    }
}

impl Drop for TransferReview {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Why a change is there, for the user.
fn reason(change: &Change) -> (String, Tone) {
    let key =
        match (change.conflict, change.action, change.local.is_some(), change.remote.is_some()) {
            (true, ..) => "remote.why_conflict",
            (false, Action::Download, false, _) => "remote.why_new_there",
            (false, Action::Download, true, _) => "remote.why_changed_there",
            (false, Action::DeleteLocal, ..) => "remote.why_removed_there",
            (false, Action::Upload, _, false) => "remote.why_new_here",
            (false, Action::Upload, _, true) => "remote.why_changed_here",
            (false, Action::DeleteRemote, ..) => "remote.why_removed_here",
        };
    let tone = match (change.conflict, change.action) {
        (true, _) | (_, Action::DeleteLocal | Action::DeleteRemote) => Tone::Danger,
        _ => Tone::Neutral,
    };
    (t(key), tone)
}

/// A removal (here or on the server).
fn is_removal(change: &Change) -> bool {
    matches!(change.action, Action::DeleteLocal | Action::DeleteRemote)
}

/// Checked by "all": anything but a conflict (decided one by one: which copy wins).
fn choosable(change: &Change) -> bool {
    !change.conflict
}

/// The icon and color key of a change's action.
fn action_icon(action: Action) -> &'static str {
    match action {
        Action::Download => "download",
        Action::Upload => "upload",
        Action::DeleteLocal | Action::DeleteRemote => "trash",
    }
}

/// Compares the scope with the server (a background thread): the changes, both copies of
/// the first conflicts.
fn compare_with_server(
    state: &State,
    direction: Direction,
    scope: &str,
    fetched_only: bool,
    cancel: &AtomicBool,
) -> Result<Vec<Row>, RemoteError> {
    let connection = &state.saved.connection;
    // One session for every listing, opened again (waiting longer each time) when the
    // server drops it; each folder listed fills the tree meanwhile (on demand).
    let learn = |dir: &str, entries: &[remote::RemoteEntry]| state.learn_listing(dir, entries);
    let mut fs =
        crate::pool::Resilient::new(state.connector.as_ref(), state.retry()).listing_to(&learn);
    let cancelled = || cancel.load(Ordering::Relaxed);
    let remote = list_remote_under(connection, &mut fs, scope, &cancelled)?;
    let local = session::local_copy(&state.saved.local);
    let here: Vec<_> = local.list().into_iter().filter(|l| in_scope(&l.path, scope)).collect();
    let manifest = state.manifest.lock().map(|m| m.clone()).unwrap_or_default();
    let hash = |path: &str| local.read(path).ok().map(|b| content_hash(&b));
    let changes = plan(connection, &manifest, &here, &remote, direction, &hash);
    let mut compared = 0;
    let mut rows = Vec::new();
    // On demand: what was never brought isn't a change (it simply stays on the server).
    let brought = |c: &Change| c.local.is_some() || manifest.get(&c.path).is_some();
    let changes = changes.into_iter().filter(|c| in_scope(&c.path, scope));
    for change in changes.filter(|c| !fetched_only || brought(c)) {
        let small = |size: Option<u64>| size.is_some_and(|s| s <= COMPARED_BYTES);
        let texts = (change.conflict
            && compared < COMPARED
            && small(change.remote.as_ref().map(|r| r.size))
            && small(change.local.as_ref().map(|l| l.size)))
        .then(|| {
            compared += 1;
            let server = fs.read(&change.path).ok()?;
            let mine = local.read(&change.path).ok()?;
            Some((
                String::from_utf8_lossy(&server).into_owned(),
                String::from_utf8_lossy(&mine).into_owned(),
            ))
        })
        .flatten();
        rows.push(Row { change, texts });
    }
    Ok(rows)
}

impl TransferReview {
    fn wake(&self) -> impl Fn() + Send + Sync + 'static {
        let waker = self.waker.clone();
        move || {
            if let Some(w) = &waker {
                w();
            }
        }
    }

    /// Compares with the server (again).
    fn start(&mut self) {
        let Some(data) = self.data.clone() else { return };
        self.started = true;
        self.rows.clear();
        self.highlight = None;
        self.scroll = 0.0;
        let Some(state) = session::session(data.project) else {
            self.phase = Phase::Failed(t("remote.not_remote"));
            return;
        };
        self.phase = Phase::Planning;
        self.cancel = Arc::default();
        let cancel = Arc::clone(&self.cancel);
        let wake = self.wake();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-plan", move || {
            let rows = compare_with_server(
                &state,
                data.direction,
                &data.scope,
                data.fetched_only,
                &cancel,
            );
            let _ = tx.send(Message::Planned(rows));
            wake();
        });
        self.rx = Some(rx);
    }

    fn chosen(&self) -> usize {
        self.rows.iter().filter(|r| r.change.chosen).count()
    }

    /// Transfers what's checked, in parallel.
    fn apply(&mut self) {
        if self.phase != Phase::Review || self.chosen() == 0 {
            return;
        }
        let Some(state) = self.data.as_ref().and_then(|d| session::session(d.project)) else {
            return;
        };
        let changes: Vec<Change> = self.rows.iter().map(|r| r.change.clone()).collect();
        let progress = Arc::clone(&self.progress);
        progress.0.store(0, Ordering::Relaxed);
        progress.1.store(self.chosen(), Ordering::Relaxed);
        let bytes = Arc::clone(&self.bytes);
        bytes.0.store(0, Ordering::Relaxed);
        let total: u64 = self
            .rows
            .iter()
            .filter(|r| r.change.chosen)
            .map(|r| match r.change.action {
                Action::Upload => r.change.local.as_ref().map_or(0, |l| l.size),
                Action::Download => r.change.remote.as_ref().map_or(0, |r| r.size),
                Action::DeleteLocal | Action::DeleteRemote => 0,
            })
            .sum();
        bytes.1.store(total, Ordering::Relaxed);
        let cancel = Arc::clone(&self.cancel);
        let wake = self.wake();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-apply", move || {
            state.running(true);
            // One operation for the Task Manager: what's checked.
            let checked: Vec<Change> = changes.iter().filter(|c| c.chosen).cloned().collect();
            let operation = state.begin(&checked);
            // Its "Stop" in the Task Manager is the view's own.
            state.stoppable(operation, Arc::clone(&cancel));
            let local = session::local_copy(&state.saved.local);
            let mut manifest = state.manifest.lock().map(|m| m.clone()).unwrap_or_default();
            let run = Run {
                // Only uploads (Push): as many as uploads may run; else as downloads.
                workers: if changes.iter().all(|c| matches!(c.action, Action::Upload)) {
                    state.limits().uploads
                } else {
                    state.limits().downloads
                },
                retry: state.retry(),
                now: &session::now,
                on_transfer: &|transfer| {
                    progress.0.fetch_add(1, Ordering::Relaxed);
                    state.record(transfer);
                    state.file_of(operation);
                },
                on_bytes: &|action, n| {
                    bytes.0.fetch_add(n, Ordering::Relaxed);
                    state.moved(action, n);
                    state.bytes_of(operation, n);
                },
                cancelled: &|| cancel.load(Ordering::Relaxed),
                parallel: &session::threads,
                pause: &std::thread::sleep,
            };
            let done = apply(&changes, state.connector.as_ref(), &local, &mut manifest, &run);
            state.keep(manifest, &done);
            state.running(false);
            let stopped = cancel.load(Ordering::Relaxed);
            state.finish(operation, &done, stopped);
            let ok = done.iter().filter(|t| t.error.is_none()).count();
            // Those cut short by the stop didn't fail.
            let cancelled = RemoteError::Cancelled.to_string();
            let failed = done
                .iter()
                .filter(|t| t.error.as_ref().is_some_and(|e| !e.contains(&cancelled)))
                .count();
            let _ = tx.send(Message::Applied { done: ok, failed, stopped });
            wake();
        });
        self.phase = Phase::Applying;
        self.rx = Some(rx);
    }

    /// The transfer going on stops (the one mid-file too).
    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn toggle(&mut self, i: usize) {
        let Some(row) = self.rows.get_mut(i) else { return };
        // A conflict is checked by choosing a side (the server's by default).
        if row.change.conflict && !row.change.chosen {
            row.change.take(true);
        } else {
            row.change.chosen = !row.change.chosen;
        }
    }

    /// The rows `of` picks: how many are checked, of how many (conflicts aren't counted:
    /// checking them all doesn't decide one).
    fn checked_of(&self, of: impl Fn(&Change) -> bool) -> (usize, usize) {
        let rows = self.rows.iter().map(|r| &r.change).filter(|c| of(c) && choosable(c));
        rows.fold((0, 0), |(on, all), c| (on + usize::from(c.chosen), all + 1))
    }

    /// The rows `of` picks all checked, or (all were already) all unchecked.
    fn check_all(&mut self, of: impl Fn(&Change) -> bool) {
        let (on, all) = self.checked_of(&of);
        let chosen = on < all;
        for row in self.rows.iter_mut().filter(|r| of(&r.change) && choosable(&r.change)) {
            row.change.chosen = chosen;
        }
    }

    /// Both copies of conflict `i`, beside the review.
    fn compare(&mut self, i: usize) -> Option<ViewerAction> {
        let data = self.data.as_ref()?;
        let row = self.rows.get(i)?;
        let (server, mine) = row.texts.clone()?;
        let path = data.root.join(&row.change.path);
        let input = diff::CompareInput {
            left: (path.clone(), server),
            right: (path, mine),
            labels: [Some(t("remote.server_copy")), Some(t("remote.local_copy"))],
            read_only: [true, false],
        };
        let title = tr!("remote.compare_tab", path = row.change.path.as_str());
        Some(ViewerAction::CompareBeside(Box::new(input), title))
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn summary(&self) -> String {
        let conflicts = self.rows.iter().filter(|r| r.change.conflict).count();
        let removals = self.rows.iter().filter(|r| is_removal(&r.change)).count();
        let mut parts = vec![tr!("remote.review_changes", count = self.rows.len())];
        if conflicts > 0 {
            parts.push(tr!("remote.review_conflicts", count = conflicts));
        }
        if removals > 0 {
            parts.push(tr!("remote.review_removals", count = removals));
        }
        parts.join(" · ")
    }
}

impl Viewer for TransferReview {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(data) = data.downcast_ref::<ReviewData>() {
            self.cancel.store(true, Ordering::Relaxed);
            self.data = Some(data.clone());
            // Compared on the next paint (with the app's waker in hand).
            self.started = false;
            self.phase = Phase::Planning;
        }
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let Ok(message) = rx.try_recv() else { return false };
        self.rx = None;
        match message {
            Message::Planned(Ok(rows)) => {
                self.rows = rows;
                self.phase = Phase::Review;
            }
            Message::Planned(Err(e)) => {
                let mut why = crate::connect_view::explain(&e);
                // The server limited the connections: said, with how many it takes.
                let hint = self.data.as_ref().and_then(|d| session::session(d.project));
                if let Some(hint) = hint.and_then(|s| s.hint()) {
                    why = format!("{why} — {hint}");
                }
                self.phase = Phase::Failed(why);
            }
            Message::Applied { done, failed, stopped } => {
                self.phase = Phase::Done { done, failed, stopped }
            }
        }
        true
    }

    /// Planning or applying: redrawn at the loading pace (its progress), never every frame.
    fn is_loading(&self) -> bool {
        matches!(self.phase, Phase::Planning | Phase::Applying)
    }

    fn purge(&mut self) {
        // The copies read for comparing go; the list stays.
        for row in &mut self.rows {
            row.texts = None;
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        let len = self.rows.len();
        // While transferring, the list is locked: Esc stops it.
        if self.phase == Phase::Applying {
            if key == ViewerKey::Escape {
                self.stop();
            }
            return true;
        }
        match key {
            ViewerKey::Down if len > 0 => {
                self.highlight = Some(self.highlight.map_or(0, |h| (h + 1).min(len - 1)));
            }
            ViewerKey::Up => self.highlight = self.highlight.map(|h| h.saturating_sub(1)),
            ViewerKey::Left | ViewerKey::Right => {
                let server = key == ViewerKey::Left;
                if let Some(row) = self.highlight.and_then(|h| self.rows.get_mut(h))
                    && row.change.conflict
                {
                    row.change.take(server);
                }
            }
            ViewerKey::Enter => match self.phase {
                Phase::Review => self.apply(),
                Phase::Done { .. } | Phase::Failed(_) => self.action = Some(ViewerAction::Close),
                _ => {}
            },
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            _ => return false,
        }
        true
    }

    fn wants_text(&self) -> bool {
        // Space checks the highlighted row.
        self.phase == Phase::Review
    }

    fn text_input(&mut self, text: &str) -> bool {
        if text == " "
            && self.phase == Phase::Review
            && let Some(i) = self.highlight
        {
            self.toggle(i);
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        let hit = self.hit(p)?;
        // While transferring, only the stop answers.
        if self.phase == Phase::Applying && hit != Hit::Stop {
            return None;
        }
        match hit {
            Hit::Stop => self.stop(),
            Hit::Check(i) => {
                self.highlight = Some(i);
                self.toggle(i);
            }
            Hit::CheckAll => self.check_all(|_| true),
            Hit::CheckRemovals => self.check_all(is_removal),
            Hit::Take(i, server) => {
                self.highlight = Some(i);
                if let Some(row) = self.rows.get_mut(i) {
                    row.change.take(server);
                }
            }
            Hit::Compare(i) => {
                self.highlight = Some(i);
                return self.compare(i);
            }
            Hit::Apply => match self.phase {
                Phase::Review => self.apply(),
                Phase::Done { .. } | Phase::Failed(_) => return Some(ViewerAction::Close),
                _ => {}
            },
            Hit::Close => return Some(ViewerAction::Close),
        }
        None
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        changed
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        // While transferring, only the stop lights up.
        let hover = self.hit(p).filter(|h| self.phase != Phase::Applying || *h == Hit::Stop);
        let changed = hover != self.hover;
        self.hover = hover;
        changed || bar
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        if !self.started {
            self.start();
        }
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let Some(data) = self.data.clone() else { return };
        let width = px(860.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = b.min_y() + px(24.0);

        // The header: which way, what (the project, a folder, a file), what was found.
        let (icon, key) = match data.direction {
            Direction::Pull => ("download", "remote.review_pull"),
            Direction::Push => ("upload", "remote.review_push"),
            Direction::Sync => ("refresh", "remote.review_sync"),
        };
        let title_font = ds.ui_font(3.0).with_weight(Font::SEMIBOLD);
        if let Some(id) = ctx.icons.id(icon) {
            let s = px(20.0);
            ui_kit::draw_icon(layer, ctx.icons, id, Rect::new(x0, y + px(4.0), s, s), p.accent);
        }
        let what = if data.scope.is_empty() { t("remote.review_project") } else { data.scope };
        let title = ctx.text.shape_line(&format!("{} · {what}", t(key)), &title_font);
        draw_text(layer, &title, x0 + px(30.0), y + title_font.size + px(2.0), p.text);
        y += px(40.0);
        let small = ds.ui_font(-1.0);
        let line = match &self.phase {
            Phase::Review | Phase::Applying | Phase::Done { .. } => self.summary(),
            _ => String::new(),
        };
        let shaped = ctx.text.shape_line(&line, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(18.0), &small), p.text_muted);
        y += px(30.0);
        // Above the list: everything, and every removal, checked at once.
        let listed = matches!(self.phase, Phase::Review | Phase::Applying | Phase::Done { .. });
        if listed && self.checked_of(|_| true).1 > 0 {
            let pen = Pen { layer, text: ctx.text, icons: ctx.icons, ds: &ds };
            self.paint_check_all(pen, x0, y);
            y += px(38.0);
        }

        let footer_h = px(64.0);
        let list = Rect::new(b.min_x(), y, b.size.width, (b.max_y() - footer_h - y).max(0.0));
        match &self.phase {
            Phase::Planning => {
                let label = t("remote.review_comparing");
                ds::loader(layer, ctx.text, &ds, x0, y + px(16.0), &ds::Loader::with_text(&label));
            }
            Phase::Failed(why) => {
                ds::empty_state(layer, ctx.text, &ds, list, why, None);
            }
            _ if self.rows.is_empty() => {
                ds::empty_state(layer, ctx.text, &ds, list, &t("remote.review_nothing"), None);
            }
            _ => self.paint_rows(ctx.scene, ctx.text, ctx.icons, &ds, Column { x0, width }, list),
        }
        self.paint_footer(ctx.scene, ctx.text, &ds, Column { x0, width }, b, footer_h);
    }
}

impl TransferReview {
    /// "Select all" and "All removals" (when there are any) from `x0`, on the line at `top`:
    /// checked, unchecked or partly, as the rows they stand for. Only while reviewing.
    fn paint_check_all(&mut self, pen: Pen<'_, '_>, x0: f32, top: f32) {
        let Pen { layer, text, ds, .. } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (line_h, font) = (px(30.0), ds.ui_font(-0.5));
        let mid = top + line_h / 2.0;
        let enabled = self.phase == Phase::Review;
        let removals = self.checked_of(is_removal);
        let mut x = x0;
        for (key, (on, all), hit) in [
            ("remote.review_check_all", self.checked_of(|_| true), Hit::CheckAll),
            ("remote.review_check_removals", removals, Hit::CheckRemovals),
        ] {
            if all == 0 {
                continue;
            }
            let label = format!("{} ({on} / {all})", t(key));
            let shaped = text.shape_line(&label, &font);
            let w = px(8.0) + px(16.0) + px(10.0) + shaped.width + px(12.0);
            let r = Rect::new(x, top, w.round(), line_h);
            if enabled && self.hover == Some(hit) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(radius::MD)));
            }
            let check =
                ds::checkbox(layer, text, ds, x + px(8.0), mid, Check::of(on, all), enabled);
            let color = if enabled { p.text } else { p.text_muted };
            draw_text(
                layer,
                &shaped,
                check.max_x() + px(10.0),
                ds::optical_baseline(top, line_h, &font),
                color,
            );
            if enabled {
                self.hits.push((r, hit));
            }
            x = r.max_x() + px(12.0);
        }
    }

    /// The changes: checked, which way, the path, why; a conflict's side and "Compare".
    fn paint_rows(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        column: Column,
        list: Rect,
    ) {
        let p = ds.palette;
        let rows = self.row_layout(text, ds, column, list);
        let row_h = rows.row_h;
        self.max_scroll = (self.rows.len() as f32 * row_h - list.size.height).max(0.0);
        // The highlighted row stays in sight (once, when it moves: the wheel stays free).
        if let Some(h) = self.highlight.filter(|h| Some(*h) != self.shown) {
            let top = h as f32 * row_h;
            self.scroll = self.scroll.min(top).max(top + row_h - list.size.height);
            self.shown = Some(h);
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let layer = scene.push_layer(Some(list));
        let mut pen = Pen { layer, text, icons, ds };
        let first = (self.scroll / row_h) as usize;
        let visible = (list.size.height / row_h) as usize + 2;
        for i in first..(first + visible).min(self.rows.len()) {
            self.paint_row(pen.lend(), &rows, i);
        }
        // More above / below: faded edges with a chevron.
        let hints = scene.push_layer(Some(list));
        ds::scroll_hints(
            hints,
            icons,
            ds,
            ds::ScrollHintsSpec {
                area: list,
                axis: ds::Axis::Vertical,
                before: self.scroll > 0.5,
                after: self.scroll < self.max_scroll - 0.5,
                background: p.editor_background,
            },
        );
        let content = self.max_scroll + list.size.height;
        // Locked while transferring: veiled.
        if self.phase == Phase::Applying {
            scene
                .push_layer(Some(list))
                .quad(Quad::filled(list, p.editor_background.with_alpha(0.5)));
        }
        self.bar.paint(scene, p, ds.scale, list, content, self.scroll.round());
    }

    /// How the rows are laid out: the conflict's controls (the same on every row) and their
    /// width, from the right edge (the scrollbar's room kept), on one line or under the path.
    fn row_layout(
        &self,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        column: Column,
        list: Rect,
    ) -> RowLayout {
        let Column { x0, width } = column;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let badge_font = ds.ui_font(-1.5);
        let right = x0 + width - px(14.0);
        let (server_label, mine_label, compare_label) =
            (t("remote.take_server"), t("remote.take_mine"), t("remote.compare"));
        let pill = |text: &mut text_layout::TextSystem, label: &str| {
            (text.shape_line(label, &ds.ui_font(0.0)).width + px(28.0)).round()
        };
        let compare_w = text.shape_line(&compare_label, &font).width + px(16.0);
        let controls_w =
            pill(text, &server_label) + px(6.0) + pill(text, &mine_label) + px(12.0) + compare_w;
        let widest_reason = self
            .rows
            .iter()
            .map(|r| text.shape_line(&reason(&r.change).0, &badge_font).width + px(14.0))
            .fold(0.0_f32, f32::max);
        let any_conflict = self.rows.iter().any(|r| r.change.conflict);
        // Narrow: a conflict's controls go under its path (two lines a row), nothing past
        // the edge.
        let path_x = x0 + px(8.0) + px(18.0) + px(38.0);
        let one_line = path_x
            + px(160.0)
            + widest_reason
            + px(16.0)
            + if any_conflict { controls_w + px(16.0) } else { 0.0 }
            <= right;
        let row_h = if one_line || !any_conflict { px(38.0) } else { px(70.0) };
        RowLayout {
            column,
            list,
            font,
            badge_font,
            right,
            server_label,
            mine_label,
            compare_label,
            compare_w,
            controls_w,
            path_x,
            one_line,
            row_h,
            line_h: px(38.0),
        }
    }

    /// A change's row: its tint, checkbox, direction, why and path, then its conflict's
    /// controls.
    fn paint_row(&mut self, pen: Pen<'_, '_>, rows: &RowLayout, i: usize) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = rows.column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (font, badge_font, right, line_h) =
            (&rows.font, &rows.badge_font, rows.right, rows.line_h);
        let row = &self.rows[i];
        let top = rows.list.min_y() + i as f32 * rows.row_h - self.scroll.round();
        let mid = top + line_h / 2.0;
        let r = Rect::new(x0, top + px(1.0), width, rows.row_h - px(2.0));
        // The whole line: selected in the accent, under the mouse in a light tint.
        let lit =
            matches!(self.hover, Some(Hit::Check(h) | Hit::Take(h, _) | Hit::Compare(h)) if h == i);
        if self.highlight == Some(i) {
            layer.quad(Quad::filled(r, p.accent.with_alpha(0.10)).rounded(px(radius::MD)));
        } else if lit {
            layer.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(radius::MD)));
        }
        let state = if row.change.chosen { Check::On } else { Check::Off };
        let check = ds::checkbox(layer, text, ds, x0 + px(8.0), mid, state, true);
        let area = Rect::new(
            check.min_x() - px(4.0),
            check.min_y() - px(4.0),
            check.size.width + px(8.0),
            check.size.height + px(8.0),
        );
        // The line checks too (its controls, pushed after, win).
        push_row_hit(&mut self.hits, r, rows.list, Hit::Check(i));
        push_row_hit(&mut self.hits, area, rows.list, Hit::Check(i));
        let color = match row.change.action {
            Action::Download => p.vcs_added,
            Action::Upload => p.accent,
            Action::DeleteLocal | Action::DeleteRemote => p.vcs_deleted,
        };
        if let Some(id) = icons.id(action_icon(row.change.action)) {
            let s = px(15.0);
            let ir = Rect::new(check.max_x() + px(12.0), mid - s / 2.0, s, s);
            ui_kit::draw_icon(layer, icons, id, ir, color);
        }
        // On one line: path · why · controls (right-aligned); else the controls below.
        let on_line = row.change.conflict && rows.one_line;
        let controls_x = if on_line { right - rows.controls_w } else { right };
        let (why, tone) = reason(&row.change);
        let reason_w = text.shape_line(&why, badge_font).width + px(14.0);
        let reason_right = if on_line { controls_x - px(16.0) } else { right };
        ds::badge(
            layer,
            text,
            ds,
            ds::BadgeSpec { label: &why, x: reason_right, mid, align_right: true, tone },
        );
        let path = ds::ellipsize_start(
            text,
            &row.change.path,
            font,
            (reason_right - reason_w - px(12.0) - rows.path_x).max(0.0),
        );
        let shaped = text.shape_line(&path, font);
        let path_color = if row.change.chosen { p.text } else { p.text_muted };
        draw_text(layer, &shaped, rows.path_x, ds::optical_baseline(top, line_h, font), path_color);
        if row.change.conflict {
            let (x, controls_mid) = if rows.one_line {
                (controls_x, mid)
            } else {
                (rows.path_x, top + line_h + px(12.0))
            };
            self.paint_conflict(Pen { layer, text, icons, ds }, rows, i, x, controls_mid);
        }
    }

    /// A conflict's controls from `x`, around `mid`: which copy wins (chosen only once
    /// decided), and "Compare" when both texts are known.
    fn paint_conflict(
        &mut self,
        pen: Pen<'_, '_>,
        rows: &RowLayout,
        i: usize,
        mut x: f32,
        mid: f32,
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let row = &self.rows[i];
        let server = row.change.chosen
            && matches!(row.change.action, Action::Download | Action::DeleteLocal);
        let mine = row.change.chosen && !server;
        for (label, on, side) in
            [(&rows.server_label, server, true), (&rows.mine_label, mine, false)]
        {
            let hit = Hit::Take(i, side);
            let pill = ds::choice(
                layer,
                text,
                ds,
                ds::ChoiceSpec { x, mid, label, selected: on, hovered: self.hover == Some(hit) },
            );
            push_row_hit(&mut self.hits, pill, rows.list, hit);
            x = pill.max_x() + px(6.0);
        }
        if row.texts.is_some() && workbench_model::presence::compare() {
            let font = &rows.font;
            let shaped = text.shape_line(&rows.compare_label, font);
            let link_h = px(28.0);
            let link = Rect::new(x + px(6.0), mid - link_h / 2.0, rows.compare_w, link_h);
            if self.hover == Some(Hit::Compare(i)) {
                layer.quad(Quad::filled(link, p.accent.with_alpha(0.10)).rounded(px(radius::MD)));
            }
            draw_text(
                layer,
                &shaped,
                link.min_x() + px(8.0),
                ds::optical_baseline(link.min_y(), link_h, font),
                p.accent,
            );
            push_row_hit(&mut self.hits, link, rows.list, Hit::Compare(i));
        }
    }

    fn paint_footer(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        column: Column,
        b: Rect,
        footer_h: f32,
    ) {
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let foot = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let layer = scene.push_layer(Some(foot));
        layer.quad(Quad::filled(
            Rect::new(foot.min_x(), foot.min_y(), foot.size.width, ds.line()),
            p.card_border,
        ));
        let mid = foot.min_y() + footer_h / 2.0;
        // As the wizard's: as wide as their label, not wider.
        let button_h = px(ds::BUTTON_H);
        let (label, enabled) = match &self.phase {
            Phase::Review => (tr!("remote.review_apply", count = self.chosen()), self.chosen() > 0),
            Phase::Planning | Phase::Applying => (t("remote.review_apply_plain"), false),
            Phase::Done { .. } | Phase::Failed(_) => (t("remote.review_close"), true),
        };
        let main_w = ds::primary_width(text, ds, &[&label]);
        let main = Rect::new(x0 + width - main_w, mid - button_h / 2.0, main_w, button_h);
        ds::primary_button(layer, text, ds, main, &label, enabled, self.hover == Some(Hit::Apply));
        self.hits.push((main, Hit::Apply));
        let mut right = main.min_x() - px(12.0);
        // Before: give up; while transferring: stop it.
        let second = match self.phase {
            Phase::Review | Phase::Planning => Some((t("remote.review_cancel"), Hit::Close)),
            Phase::Applying if self.cancel.load(Ordering::Relaxed) => {
                Some((t("remote.review_stopping"), Hit::Stop))
            }
            Phase::Applying => Some((t("remote.review_stop"), Hit::Stop)),
            _ => None,
        };
        if let Some((label, hit)) = second {
            let font = ds::button_font(ds);
            let w = (text.shape_line(&label, &font).width + px(36.0)).round();
            let button = Rect::new(right - w, main.min_y(), w, button_h);
            let fill =
                if self.hover == Some(hit) { p.text.with_alpha(0.08) } else { Rgba::TRANSPARENT };
            layer.quad(
                Quad::filled(button, fill)
                    .rounded(px(radius::MD))
                    .bordered(ds.line(), p.card_border),
            );
            let shaped = text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                button.min_x() + (button.size.width - shaped.width) / 2.0,
                ds::optical_baseline(button.min_y(), button_h, &font),
                if hit == Hit::Stop { p.danger } else { p.text },
            );
            self.hits.push((button, hit));
            right = button.min_x() - px(12.0);
        }
        let room = (right - x0).max(0.0);
        let small = ds.ui_font(-0.5);
        match &self.phase {
            Phase::Applying => {
                // Files and bytes (the bar follows the bytes; nothing to move: the files).
                let (done, total) = (
                    self.progress.0.load(Ordering::Relaxed),
                    self.progress.1.load(Ordering::Relaxed),
                );
                let (moved, size) =
                    (self.bytes.0.load(Ordering::Relaxed), self.bytes.1.load(Ordering::Relaxed));
                let mut label = tr!("remote.review_applying", done = done, total = total);
                if size > 0 {
                    let (moved, size) = (i18n::bytes(moved.min(size)), i18n::bytes(size));
                    label = format!("{label} · {moved} / {size}");
                }
                let fraction = if size > 0 {
                    Some(moved as f32 / size as f32)
                } else if total > 0 {
                    Some(done as f32 / total as f32)
                } else {
                    None
                };
                let label = ds::ellipsize(text, &label, &small, room - px(30.0));
                ds::loader(layer, text, ds, x0, mid - px(9.0), &ds::Loader::with_text(&label));
                let bar = Rect::new(x0, mid + px(9.0), (room - px(12.0)).max(0.0), px(4.0));
                ds::ProgressBar::from(fraction).paint_bar(layer, ds, bar);
            }
            Phase::Done { done, failed, stopped } => {
                let (message, color) = if *stopped {
                    (tr!("remote.review_stopped", count = *done), p.warning)
                } else if *failed == 0 {
                    (tr!("remote.review_done", count = *done), p.accent)
                } else {
                    (tr!("remote.review_done_failed", count = *done, failed = *failed), p.danger)
                };
                let message = ds::ellipsize(text, &message, &small, room);
                let shaped = text.shape_line(&message, &small);
                draw_text(
                    layer,
                    &shaped,
                    x0,
                    ds::optical_baseline(foot.min_y(), footer_h, &small),
                    color,
                );
            }
            _ => {}
        }
    }
}

/// How the change rows are laid out (measured once a frame): their column and list, fonts,
/// the conflict's controls (labels and widths), where the path starts, one line or two.
struct RowLayout {
    column: Column,
    list: Rect,
    font: Font,
    badge_font: Font,
    /// Where the row's content ends (the scrollbar's room kept).
    right: f32,
    server_label: String,
    mine_label: String,
    compare_label: String,
    compare_w: f32,
    controls_w: f32,
    path_x: f32,
    /// The conflict's controls fit on the path's line.
    one_line: bool,
    row_h: f32,
    line_h: f32,
}

/// A row's hit, only its part inside the list (a row under the header or the footer must not
/// take their clicks).
fn push_row_hit(hits: &mut Vec<(Rect, Hit)>, r: Rect, list: Rect, hit: Hit) {
    if let Some(r) = r.intersect(&list) {
        hits.push((r, hit));
    }
}

#[cfg(test)]
#[path = "../tests/unit/review_view.rs"]
mod tests;
