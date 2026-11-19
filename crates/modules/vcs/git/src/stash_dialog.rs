//! The stash dialogs (module modals, closed only by ×, Esc or their buttons):
//! - Stash Changes…: a message (git's own when left empty), whether what's staged stays
//!   staged, whether the untracked files go too, whether only what's staged goes (a git that
//!   can't greys it out, and options that don't go together grey each other out); of chosen
//!   files only when opened on them;
//! - Apply Stash…: the stash brought back, dropped after when chosen.
//!
//! What it needs is read in the background before it opens; the work runs on a worker and
//! the dialog closes once it's done, git's refusal shown in it.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use ui_components::ds::{self, Ctx};
use ui_components::{
    KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::{TextInput, draw_text};
use version_control::{Repository, Stash, StashOptions, VcsError};
use workbench_model::{CommandHandler, Completion, Notification, WorkbenchHost};

/// The extension of the dialogs' view.
pub const EXTENSION: &str = "view-git-stash";

/// What a Stash Changes… starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StashPreset {
    /// Only what's staged ticked.
    pub staged_only: bool,
    /// Only these files (none: all).
    pub paths: Vec<String>,
}

/// Which dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashAsked {
    /// The changes put aside: on `branch`; `staged`: this git stashes what's staged alone.
    Push { branch: String, staged: bool, preset_staged: bool, paths: Vec<String> },
    /// A stash brought back.
    Apply(Stash),
}

/// What a dialog is opened with.
#[derive(Clone)]
pub struct StashRequest {
    pub repo: Arc<dyn Repository>,
    pub asked: StashAsked,
}

#[derive(Debug)]
pub(crate) struct StashDialogProvider;

impl ViewerProvider for StashDialogProvider {
    fn id(&self) -> &'static str {
        "git.stash"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(StashDialog::default()))
    }
}

/// Stash Changes…, opened on `preset` (what's read first, in the background).
pub(crate) fn open_push(host: &mut dyn WorkbenchHost, preset: StashPreset) -> bool {
    let Some(repo) = host.repository() else { return false };
    host.spawn(Box::new(move || {
        let branch = repo.branch().unwrap_or_else(|| "HEAD".into());
        let staged = repo.stashes_staged();
        let anything = repo.staged_files().map(|f| !f.is_empty()).unwrap_or(false);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            if !anything {
                host.notify(Notification::info(t("git.stash_nothing")));
                return;
            }
            let asked = StashAsked::Push {
                branch,
                staged,
                preset_staged: preset.staged_only && staged,
                paths: preset.paths,
            };
            host.open_modal(&t("git.stash_title"), EXTENSION, &StashRequest { repo, asked });
        }) as Completion
    }));
    true
}

/// Apply Stash… on `stash`, as a view's action.
pub(crate) fn apply_action(stash: Stash) -> ViewerAction {
    ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
        let Some(repo) = host.repository() else { return };
        let request = StashRequest { repo, asked: StashAsked::Apply(stash) };
        host.open_modal(&t("git.stash_apply_title"), EXTENSION, &request);
    }))
}

/// The Git menu's Stash Changes….
pub(crate) struct StashChanges;

impl CommandHandler for StashChanges {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        open_push(host, StashPreset::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Message,
    KeepIndex,
    Untracked,
    StagedOnly,
    Pop,
}

#[derive(Default)]
pub struct StashDialog {
    request: Option<StashRequest>,
    message: TextInput,
    keep_index: bool,
    untracked: bool,
    staged_only: bool,
    /// Apply: dropped after.
    pop: bool,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    tips: Vec<(Rect, String)>,
    action: Option<ViewerAction>,
    running: Option<Receiver<Result<(), VcsError>>>,
    failed: Option<String>,
    done: bool,
    notes: Vec<Notification>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for StashDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StashDialog").field("done", &self.done).finish_non_exhaustive()
    }
}

impl StashDialog {
    fn pushing(&self) -> bool {
        matches!(self.request.as_ref().map(|r| &r.asked), Some(StashAsked::Push { .. }))
    }

    /// The options as ticked.
    pub fn options(&self) -> StashOptions {
        let paths = match self.request.as_ref().map(|r| &r.asked) {
            Some(StashAsked::Push { paths, .. }) => paths.clone(),
            _ => Vec::new(),
        };
        let message = self.message.text().trim();
        StashOptions {
            message: (!message.is_empty()).then(|| message.to_owned()),
            keep_index: self.keep_index,
            untracked: self.untracked,
            staged_only: self.staged_only,
            paths,
        }
    }

    /// Whether `hit`'s option can be ticked now (one that doesn't go with what's ticked
    /// can't; nor only-staged on a git without it).
    fn can_tick(&self, hit: Hit) -> bool {
        if hit == Hit::Pop {
            return true;
        }
        let Some(StashAsked::Push { staged, paths, .. }) = self.request.as_ref().map(|r| &r.asked)
        else {
            return false;
        };
        match hit {
            Hit::KeepIndex | Hit::Untracked => !self.staged_only,
            Hit::StagedOnly => *staged && !self.keep_index && !self.untracked && paths.is_empty(),
            _ => true,
        }
    }

    fn tick(&mut self, hit: Hit) {
        if !self.can_tick(hit) {
            return;
        }
        match hit {
            Hit::KeepIndex => self.keep_index = !self.keep_index,
            Hit::Untracked => self.untracked = !self.untracked,
            Hit::StagedOnly => self.staged_only = !self.staged_only,
            Hit::Pop => self.pop = !self.pop,
            _ => {}
        }
    }

    /// The stash made or applied on a worker; the dialog closes once it's done.
    pub fn go(&mut self) -> bool {
        if self.running.is_some() {
            return false;
        }
        let Some(r) = self.request.clone() else { return false };
        let options = self.options();
        if options.check().is_err() {
            return false;
        }
        let pop = self.pop;
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-commit", move || {
            let result = match &r.asked {
                StashAsked::Push { .. } => r.repo.stash_push(&options),
                StashAsked::Apply(stash) => r.repo.stash_apply(&stash.id, pop),
            };
            crate::refs_store::moved(r.repo.root());
            let _ = tx.send(result);
            if let Some(wake) = waker {
                wake();
            }
        });
        self.running = Some(rx);
        self.failed = None;
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn done_note(&self) -> String {
        if self.pushing() {
            t("git.stash_made")
        } else if self.pop {
            t("git.stash_popped")
        } else {
            t("git.stash_applied")
        }
    }

    /// A checkbox row at `y` with `label`: its rectangle.
    fn check_row(
        &mut self,
        (layer, text, ds): (&mut render_api::Layer, &mut text_layout::TextSystem, &Ctx<'_>),
        (x, y, width): (f32, f32, f32),
        (hit, on, label): (Hit, bool, &str),
    ) -> Rect {
        let px = |v: f32| ds.px(v);
        let row = Rect::new(x, y, width, px(28.0));
        let enabled = self.can_tick(hit);
        let state = if on { ds::Check::On } else { ds::Check::Off };
        let mid = row.min_y() + row.size.height / 2.0;
        let r = ds::checkbox(layer, text, ds, row.min_x(), mid, state, enabled);
        let font = ds.ui_font(-0.5);
        let shaped = text.shape_line(label, &font);
        let color = if enabled { ds.palette.text } else { ds.palette.text_muted };
        let baseline = ds::optical_baseline(row.min_y(), row.size.height, &font);
        draw_text(layer, &shaped, r.max_x() + px(8.0), baseline, color);
        self.hits.push((row, hit));
        row
    }
}

impl Viewer for StashDialog {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<StashRequest>() else { return };
        if let StashAsked::Push { preset_staged, .. } = &request.asked {
            self.staged_only = *preset_staged;
        }
        self.request = Some(request.clone());
    }

    fn subtitle(&self) -> Option<String> {
        Some(match &self.request.as_ref()?.asked {
            StashAsked::Push { branch, paths, .. } if paths.is_empty() => {
                tr!("git.stash_on", name = branch.as_str())
            }
            StashAsked::Push { paths, .. } => tr!("git.stash_files", count = paths.len()),
            StashAsked::Apply(stash) => stash.message.clone(),
        })
    }

    fn modal_height(&self) -> Option<f32> {
        Some(if self.pushing() { 220.0 } else { 110.0 })
    }

    fn modal_width(&self) -> Option<f32> {
        Some(560.0)
    }

    /// Stash / Apply (↵), Cancel — in the window's footer; Stash greyed while it runs or
    /// while the options don't go together.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let can = self.running.is_none() && self.options().check().is_ok();
        vec![
            ModalButton::main("go", self.go_label(), can),
            ModalButton::other("cancel", t("git.ref_cancel")),
        ]
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(if self.pushing() { "archive-down" } else { "archive-up" })
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], self.go_label())]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        self.pushing()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if !self.pushing() {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.message.insert(&typed);
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        // One line: its breaks made spaces.
        let line = text.split(['\n', '\r']).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        self.text_input(&line)
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.running else { return false };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.running = None;
                self.notes.push(Notification::success(self.done_note()));
                self.done = true;
                true
            }
            Ok(Err(e)) => {
                self.running = None;
                self.failed = Some(crate::error_message(&e));
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running = None;
                true
            }
        }
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notes)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        let (r, tip) = self.tips.iter().find(|(r, _)| r.contains(p))?;
        Some(ui_components::Tip::new(*r, tip.clone()))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        self.tips.clear();
        let Some(asked) = self.request.as_ref().map(|r| r.asked.clone()) else { return };
        let pad = px(20.0);
        let layer = scene.push_layer(Some(b));
        let width = b.size.width - 2.0 * pad;
        let x = b.min_x() + pad;
        let mut y = b.min_y() + px(10.0);
        match &asked {
            StashAsked::Push { branch, staged, .. } => {
                let field = Rect::new(x, y, width, px(36.0));
                let style = ds::field_style(&ds, ds.ui_font(-0.5), false);
                let placeholder = tr!("git.stash_message_placeholder", name = branch.as_str());
                self.message.paint(layer, text, field, true, &placeholder, &style);
                self.hits.push((field, Hit::Message));
                y = field.max_y() + px(14.0);
                let pen = (&mut *layer, &mut *text, &ds);
                self.check_row(
                    pen,
                    (x, y, width),
                    (Hit::KeepIndex, self.keep_index, &t("git.stash_keep_index")),
                );
                y += px(30.0);
                let pen = (&mut *layer, &mut *text, &ds);
                self.check_row(
                    pen,
                    (x, y, width),
                    (Hit::Untracked, self.untracked, &t("git.stash_untracked")),
                );
                y += px(30.0);
                let pen = (&mut *layer, &mut *text, &ds);
                let row = self.check_row(
                    pen,
                    (x, y, width),
                    (Hit::StagedOnly, self.staged_only, &t("git.stash_staged_only")),
                );
                if !*staged {
                    self.tips.push((row, t("git.stash_staged_needs_git")));
                } else if !self.can_tick(Hit::StagedOnly) {
                    self.tips.push((row, t("git.stash_staged_exclusive")));
                }
                y += px(36.0);
            }
            StashAsked::Apply(_) => {
                let pen = (&mut *layer, &mut *text, &ds);
                self.check_row(
                    pen,
                    (x, y, width),
                    (Hit::Pop, self.pop, &t("git.stash_drop_after")),
                );
                y += px(36.0);
            }
        }
        // What git answered, or what happens.
        let small = ds.ui_font(-1.0);
        let (note, color) = match &self.failed {
            Some(failed) => (failed.clone(), p.danger),
            None if self.pushing() => (t("git.stash_note"), p.text_muted),
            None => (t("git.stash_apply_note"), p.text_muted),
        };
        for line in ds::wrap(text, &note, &small, width, 2) {
            let shaped = text.shape_line(&line, &small);
            draw_text(layer, &shaped, x, ds::optical_baseline(y, px(20.0), &small), color);
            y += px(20.0);
        }
    }

    fn is_animating(&self) -> bool {
        self.running.is_some()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        match self.hit(p) {
            Some(Hit::Message) => self.message.press(p.x, clicks),
            Some(hit) if self.running.is_none() => self.tick(hit),
            _ => {}
        }
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.message.extend_to(p.x);
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Message => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let input = &mut self.message;
        match key {
            ViewerKey::Enter => {
                self.go();
            }
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            _ => return false,
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.message.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "go" => self.go(),
            "cancel" => {
                self.action = Some(ViewerAction::Close);
                true
            }
            "select_all" => {
                self.message.select_all();
                true
            }
            "undo" => self.message.undo(),
            "redo" => self.message.redo(),
            "cut" => {
                let had = !self.message.selected_text().is_empty();
                if had {
                    self.message.insert("");
                }
                had
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

impl StashDialog {
    fn go_label(&self) -> String {
        if self.pushing() { t("git.stash_go") } else { t("git.stash_apply_go") }
    }
}

#[cfg(test)]
#[path = "../tests/unit/stash_dialog.rs"]
mod tests;
