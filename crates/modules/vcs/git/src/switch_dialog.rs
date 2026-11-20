//! "Your changes are in the way": the modal shown when switching would lose changes (the
//! files being worked on differ on the target too). It lists those files with their states
//! and offers to put the work aside and switch (the default, `↵`), to throw the changes away
//! and switch (asked once more: the button arms first), to commit first, or to cancel. The
//! work runs on a worker; the dialog closes once it's done, git's refusal shown in it.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use ui_components::control::{Control, Controls, Scrolled};
use ui_components::ds::{self, Ctx};
use ui_components::{
    EdgeScrollbar, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::draw_text;
use version_control::{Repository, SwitchTarget, VcsError};
use workbench_model::Notification;

use crate::switch::InTheWay;

/// The extension of the dialog's view.
pub const EXTENSION: &str = "view-git-switch";

/// What the dialog is opened with.
#[derive(Clone)]
pub struct SwitchRequest {
    pub repo: Arc<dyn Repository>,
    pub target: SwitchTarget,
    /// What's switched to, for the user.
    pub label: String,
    pub files: Vec<InTheWay>,
}

#[derive(Debug)]
pub(crate) struct SwitchDialogProvider;

impl ViewerProvider for SwitchDialogProvider {
    fn id(&self) -> &'static str {
        "git.switch"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SwitchDialog::default()))
    }
}

/// A footer's button (its command id is [`Hit::id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Stash,
    Discard,
    Commit,
    Cancel,
}

impl Hit {
    const ALL: [Self; 4] = [Self::Stash, Self::Discard, Self::Commit, Self::Cancel];

    fn id(self) -> &'static str {
        match self {
            Self::Stash => "stash",
            Self::Discard => "discard",
            Self::Commit => "commit_first",
            Self::Cancel => "cancel",
        }
    }
}

/// What the worker did.
enum Done {
    /// Switched; the work put aside (its stash's commit) when it was.
    Switched(Option<String>),
    Failed(VcsError),
}

/// A file's row height (logical pixels).
const ROW_H: f32 = 26.0;
/// The rows shown before the list scrolls.
const ROWS_SHOWN: usize = 8;

#[derive(Default)]
pub struct SwitchDialog {
    request: Option<SwitchRequest>,
    /// The discard button was pressed once: the next press throws the changes away.
    armed: bool,
    running: Option<Receiver<Done>>,
    failed: Option<String>,
    done: bool,
    notes: Vec<Notification>,
    action: Option<ViewerAction>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    list: Rect,
    scroll: f32,
    /// How far the files scroll, and where the dialog was drawn (the wheel is the files'
    /// anywhere over it): from the last paint.
    max_scroll: f32,
    bounds: Rect,
    bar: EdgeScrollbar,
    badges: Vec<(Rect, version_control::FileState)>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for SwitchDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwitchDialog").field("done", &self.done).finish_non_exhaustive()
    }
}

impl SwitchDialog {
    /// The switch run on a worker: the work put aside first when `stash`, else thrown away.
    pub fn go(&mut self, stash: bool) -> bool {
        if self.running.is_some() {
            return false;
        }
        let Some(r) = self.request.clone() else { return false };
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-commit", move || {
            let done = if stash {
                let options = crate::switch::put_aside_options(&r.label);
                let before = r.repo.stashes().first().map(|s| s.id.clone());
                let result = r.repo.stash_push(&options).and_then(|()| {
                    let made = r.repo.stashes().first().map(|s| s.id.clone());
                    let made = made.filter(|id| Some(id) != before.as_ref());
                    match r.repo.switch(&r.target, false) {
                        Ok(()) => Ok(made),
                        Err(e) => {
                            // Not switched: the work put back where it was.
                            if let Some(id) = &made {
                                let _ = r.repo.stash_apply(id, true);
                            }
                            Err(e)
                        }
                    }
                });
                match result {
                    Ok(made) => Done::Switched(made),
                    Err(e) => Done::Failed(e),
                }
            } else {
                match r.repo.switch(&r.target, true) {
                    Ok(()) => Done::Switched(None),
                    Err(e) => Done::Failed(e),
                }
            };
            crate::refs_store::moved(r.repo.root());
            let _ = tx.send(done);
            if let Some(wake) = waker {
                wake();
            }
        });
        self.running = Some(rx);
        self.failed = None;
        true
    }

    fn press_hit(&mut self, hit: Hit) {
        match hit {
            Hit::Stash => {
                self.go(true);
            }
            Hit::Discard if self.armed => {
                self.go(false);
            }
            Hit::Discard => self.armed = true,
            Hit::Commit => {
                self.action = Some(ViewerAction::RunCommand(crate::COMMIT.0.to_owned()));
                self.done = true;
            }
            Hit::Cancel => self.action = Some(ViewerAction::Close),
        }
    }

    fn content(&self) -> f32 {
        self.request.as_ref().map_or(0, |r| r.files.len()) as f32 * ROW_H
    }

    /// Its parts used as controls (the files' scrollbar with the offset it moves, the wheel
    /// anywhere over the dialog), the topmost first, with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut files = Scrolled {
            bar: &mut self.bar,
            offset: &mut self.scroll,
            area: self.bounds,
            max: self.max_scroll,
        };
        let mut parts: [&mut dyn Control; 1] = [&mut files];
        f(&mut self.controls, &mut parts)
    }
}

impl Viewer for SwitchDialog {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<SwitchRequest>() else { return };
        self.request = Some(request.clone());
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(tr!("git.switch_blocked_subtitle", name = r.label.as_str(), count = r.files.len()))
    }

    fn modal_height(&self) -> Option<f32> {
        let rows = self.request.as_ref()?.files.len().clamp(1, ROWS_SHOWN);
        Some(110.0 + rows as f32 * ROW_H + 8.0)
    }

    fn modal_width(&self) -> Option<f32> {
        Some(620.0)
    }

    /// Right to left: Stash and Switch (↵), Discard and Switch (in the danger color once
    /// armed), Cancel; Commit First apart. Greyed while the switch runs.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let free = self.running.is_none();
        let discard =
            if self.armed { t("git.switch_discard_sure") } else { t("git.switch_discard_go") };
        vec![
            ModalButton::main(Hit::Stash.id(), t("git.switch_stash_go"), free),
            ModalButton::other(Hit::Discard.id(), discard).dangerous(self.armed).enabled(free),
            ModalButton::other(Hit::Cancel.id(), t("git.ref_cancel")),
            ModalButton::other(Hit::Commit.id(), t("git.switch_commit_first"))
                .enabled(free)
                .separated(),
        ]
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("arrow-left-right")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], t("git.switch_stash_go")), (vec!["esc"], t("git.ref_cancel"))]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.running else { return false };
        match rx.try_recv() {
            Ok(Done::Switched(made)) => {
                self.running = None;
                let label = self.request.as_ref().map(|r| r.label.clone()).unwrap_or_default();
                if let (Some(id), Some(r)) = (&made, &self.request) {
                    crate::switch::remember_put_aside(r.repo.root(), id.clone());
                }
                self.notes.push(crate::switch::switched(&label, 0, made.is_some()));
                self.done = true;
                true
            }
            Ok(Done::Failed(e)) => {
                self.running = None;
                self.armed = false;
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

    fn is_animating(&self) -> bool {
        self.running.is_some()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notes)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        let (r, state) = self.badges.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(ui_components::Tip::new(*r, crate::files_tree::state_name(*state)))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.badges.clear();
        let Some(r) = self.request.clone() else { return };
        let pad = px(20.0);
        let layer = scene.push_layer(Some(b));
        let small = ds.ui_font(-1.0);
        // What happens, or what git answered.
        let (note, color) = match &self.failed {
            Some(failed) => (failed.clone(), p.danger),
            None => (t("git.switch_blocked_note"), p.text_muted),
        };
        let width = b.size.width - 2.0 * pad;
        let lines = ds::wrap(text, &note, &small, width, 2);
        let mut y = b.min_y() + px(6.0);
        for line in &lines {
            let shaped = text.shape_line(line, &small);
            draw_text(
                layer,
                &shaped,
                b.min_x() + pad,
                ds::optical_baseline(y, px(20.0), &small),
                color,
            );
            y += px(20.0);
        }
        y += px(8.0);
        // The files, in a box that scrolls.
        let rows = r.files.len().clamp(1, ROWS_SHOWN) as f32;
        let list = Rect::new(b.min_x() + pad, y, width, rows * px(ROW_H) + px(8.0));
        layer.quad(
            render_api::Quad::filled(list, p.text.with_alpha(0.03))
                .rounded(px(ds::radius::MD))
                .bordered(px(1.0).max(1.0), p.card_border),
        );
        self.list = list;
        let content = self.content() * scale + px(8.0);
        self.bounds = b;
        self.max_scroll = (content - list.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let rows_layer = scene.push_layer(Some(list));
        for (i, file) in r.files.iter().enumerate() {
            let top = list.min_y() + px(4.0) + i as f32 * px(ROW_H) - self.scroll;
            if top + px(ROW_H) < list.min_y() || top > list.max_y() {
                continue;
            }
            let row = Rect::new(list.min_x() + px(8.0), top, list.size.width - px(16.0), px(ROW_H));
            let side = px(ds::CHANGE_BADGE);
            let badge = Rect::new(
                row.min_x().round(),
                (row.min_y() + (row.size.height - side) / 2.0).round(),
                side,
                side,
            );
            let state = crate::files_tree::change_state(file.state);
            ds::change_badge(rows_layer, icons, &ds, badge, state);
            self.badges.push((badge.intersect(&list).unwrap_or_default(), file.state));
            let line = Rect::new(
                badge.max_x() + px(4.0),
                row.min_y(),
                row.size.width - side,
                row.size.height,
            );
            let path = file.path.as_str();
            let color = p.text_muted;
            ds::file_row(
                rows_layer,
                text,
                &ds,
                line,
                ds::FileRow { path, mark: "", color, tag: None },
            );
        }
        self.bar.set_auto_hiding(true);
        self.bar.paint(scene, p, scale, list, content, self.scroll);
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press(parts, p, clicks));
        None
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    /// The files', wherever the mouse is over the dialog (told no point: its middle).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let over = self.bounds.center();
        self.with_controls(|c, parts| c.scroll(parts, over, dx, dy)).is_some()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn cursor_at(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Arrow)
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(hit) = Hit::ALL.into_iter().find(|h| h.id() == id) else { return false };
        if self.running.is_none() {
            self.press_hit(hit);
        }
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter if self.running.is_none() => {
                self.go(true);
            }
            ViewerKey::Escape if self.running.is_none() => {
                self.action = Some(ViewerAction::Close);
            }
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/switch_dialog.rs"]
mod tests;
