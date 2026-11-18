//! The Git dialogs naming a reference (module modals, closed only by ×, Esc or their
//! buttons): New Branch… (its name, where it starts — `HEAD`, a commit or a branch — and
//! whether it's checked out after), Rename Branch… (a local branch) and New Tag… (its name,
//! the commit it's on, a message making it annotated). The name is checked as git checks
//! references' names, and against those already there; the write runs in the background
//! (the installed `git`, as commits do) and the dialog closes once it's done.
//!
//! A dialog starts from what it's asked with (a commit or a branch chosen in the Log), else
//! `HEAD`, the current branch. What it needs is read in the background before it opens.

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
use version_control::{RefKind, RefNameError, Repository, VcsError, check_ref_name};
use workbench_model::{CommandHandler, Completion, Notification, WorkbenchHost};

/// The extension of the dialogs' view.
pub const EXTENSION: &str = "view-git-ref";

/// What a dialog starts from, chosen in the Log (a commit's or a branch's menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A commit: its id and what's shown of it.
    Commit { id: String, short: String },
    /// A branch (local or remote) or a tag.
    Reference { name: String, local_branch: bool },
}

/// Which dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKindAsked {
    /// From a commit or a branch (`None`: `HEAD`), shown as `from_label`.
    NewBranch {
        from: Option<String>,
        from_label: String,
    },
    RenameBranch {
        old: String,
    },
    /// On a commit (`None`: `HEAD`), shown as `on_label`.
    NewTag {
        on: Option<String>,
        on_label: String,
    },
}

/// What the dialog is opened with.
#[derive(Clone)]
pub struct RefRequest {
    pub repo: Arc<dyn Repository>,
    pub kind: RefKindAsked,
    /// The names already taken (the local branches; the tags).
    pub taken: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct RefDialogProvider;

impl ViewerProvider for RefDialogProvider {
    fn id(&self) -> &'static str {
        "git.ref"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RefDialog::default()))
    }
}

/// The names taken for `kind`: the local branches (a branch), the tags (a tag).
fn taken(repo: &dyn Repository, tag: bool) -> Vec<String> {
    let kind = if tag { RefKind::Tag } else { RefKind::Branch };
    repo.refs().into_iter().filter(|r| r.kind == kind).map(|r| r.name).collect()
}

/// What was read, as a dialog to open (`None`: nothing to open it on).
type Reading = fn(&dyn Repository, Option<Target>) -> Option<(RefKindAsked, bool)>;

/// Reads what a dialog needs in the background, then opens it (or says why it can't).
fn open(host: &mut dyn WorkbenchHost, target: Option<Target>, read: Reading) -> bool {
    let Some(repo) = host.repository() else { return false };
    host.spawn(Box::new(move || {
        let asked = read(repo.as_ref(), target);
        let taken = asked.as_ref().map(|(_, tag)| taken(repo.as_ref(), *tag));
        Box::new(move |host: &mut dyn WorkbenchHost| {
            let (Some((kind, _)), Some(taken)) = (asked, taken) else {
                host.notify(Notification::info(t("git.no_branch_to_rename")));
                return;
            };
            let title = match kind {
                RefKindAsked::NewBranch { .. } => t("git.new_branch_title"),
                RefKindAsked::RenameBranch { .. } => t("git.rename_branch_title"),
                RefKindAsked::NewTag { .. } => t("git.new_tag_title"),
            };
            host.open_modal(&title, EXTENSION, &RefRequest { repo, kind, taken });
        }) as Completion
    }));
    true
}

fn new_branch_on(repo: &dyn Repository, target: Option<Target>) -> Option<(RefKindAsked, bool)> {
    let (from, from_label) = match target {
        Some(Target::Commit { id, short }) => (Some(id), short),
        Some(Target::Reference { name, .. }) => (Some(name.clone()), name),
        None => (None, repo.branch().map_or_else(|| "HEAD".into(), |b| format!("HEAD ({b})"))),
    };
    Some((RefKindAsked::NewBranch { from, from_label }, false))
}

fn rename_on(repo: &dyn Repository, target: Option<Target>) -> Option<(RefKindAsked, bool)> {
    let old = match target {
        Some(Target::Reference { name, local_branch: true }) => Some(name),
        _ => repo.branch().filter(|b| repo.branches().contains(b)),
    };
    Some((RefKindAsked::RenameBranch { old: old? }, false))
}

fn new_tag_on(_repo: &dyn Repository, target: Option<Target>) -> Option<(RefKindAsked, bool)> {
    let (on, on_label) = match target {
        Some(Target::Commit { id, short }) => (Some(id), short),
        Some(Target::Reference { name, .. }) => (Some(name.clone()), name),
        None => (None, "HEAD".into()),
    };
    Some((RefKindAsked::NewTag { on, on_label }, true))
}

/// New Branch…, from `target`, else `HEAD`.
pub(crate) fn new_branch(host: &mut dyn WorkbenchHost, target: Option<Target>) -> bool {
    open(host, target, new_branch_on)
}

/// Rename Branch…, of `target`, else the current one.
pub(crate) fn rename_branch(host: &mut dyn WorkbenchHost, target: Option<Target>) -> bool {
    open(host, target, rename_on)
}

/// New Tag…, on `target`, else `HEAD`.
pub(crate) fn new_tag(host: &mut dyn WorkbenchHost, target: Option<Target>) -> bool {
    open(host, target, new_tag_on)
}

/// A command of the Git menu opening one of the dialogs on what's current.
pub(crate) struct AskRef(pub fn(&mut dyn WorkbenchHost, Option<Target>) -> bool);

impl CommandHandler for AskRef {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        (self.0)(host, None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Name,
    Message,
    Checkout,
    Go,
    Cancel,
}

/// The field with the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Field {
    #[default]
    Name,
    Message,
}

#[derive(Default)]
pub struct RefDialog {
    request: Option<RefRequest>,
    name: TextInput,
    message: TextInput,
    field: Field,
    checkout: bool,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    running: Option<Receiver<Result<(), VcsError>>>,
    /// What git answered when it failed.
    failed: Option<String>,
    done: bool,
    notes: Vec<Notification>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for RefDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefDialog").field("done", &self.done).finish_non_exhaustive()
    }
}

impl RefDialog {
    fn is_tag(&self) -> bool {
        matches!(self.request.as_ref().map(|r| &r.kind), Some(RefKindAsked::NewTag { .. }))
    }

    /// Why the name typed can't be given (`None`: it can).
    pub fn problem(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        let name = self.name.text().trim();
        if let RefKindAsked::RenameBranch { old } = &r.kind
            && name == old
        {
            return Some(t("git.ref_same"));
        }
        let error = check_ref_name(name)
            .err()
            .or_else(|| r.taken.iter().any(|t| t == name).then_some(RefNameError::Exists))?;
        Some(match error {
            RefNameError::Empty => t("git.ref_empty"),
            RefNameError::BadCharacter => t("git.ref_bad_character"),
            RefNameError::BadShape => t("git.ref_bad_shape"),
            RefNameError::Exists => {
                if self.is_tag() {
                    t("git.ref_tag_exists")
                } else {
                    t("git.ref_branch_exists")
                }
            }
        })
    }

    /// The name given (when it can be): the write runs; the dialog closes once it's done.
    pub fn give(&mut self) -> bool {
        if self.problem().is_some() || self.running.is_some() {
            return false;
        }
        let Some(r) = self.request.clone() else { return false };
        let name = self.name.text().trim().to_owned();
        let message = self.message.text().trim().to_owned();
        let checkout = self.checkout;
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-commit", move || {
            let result = match &r.kind {
                RefKindAsked::NewBranch { from, .. } => {
                    r.repo.create_branch(&name, from.as_deref(), checkout)
                }
                RefKindAsked::RenameBranch { old } => r.repo.rename_branch(old, &name),
                RefKindAsked::NewTag { on, .. } => {
                    let message = (!message.is_empty()).then_some(message.as_str());
                    r.repo.create_tag(&name, on.as_deref(), message)
                }
            };
            // A branch or a tag made, renamed: every view of the references shows it.
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

    fn input(&mut self) -> &mut TextInput {
        match self.field {
            Field::Name => &mut self.name,
            Field::Message => &mut self.message,
        }
    }

    /// What it reports once done.
    fn done_note(&self) -> String {
        let name = self.name.text().trim();
        match self.request.as_ref().map(|r| &r.kind) {
            Some(RefKindAsked::NewBranch { .. }) => tr!("git.branch_created", name = name),
            Some(RefKindAsked::RenameBranch { .. }) => tr!("git.branch_renamed", name = name),
            _ => tr!("git.tag_created", name = name),
        }
    }
}

/// The dialogs' content heights (logical pixels).
fn height(kind: &RefKindAsked) -> f32 {
    match kind {
        RefKindAsked::NewBranch { .. } => 210.0,
        RefKindAsked::RenameBranch { .. } => 150.0,
        RefKindAsked::NewTag { .. } => 240.0,
    }
}

impl Viewer for RefDialog {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<RefRequest>() else { return };
        let name = match &request.kind {
            RefKindAsked::RenameBranch { old } => old.clone(),
            _ => String::new(),
        };
        self.name = TextInput::new(&name);
        self.name.select_all();
        self.checkout = true;
        self.request = Some(request.clone());
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(match &r.kind {
            RefKindAsked::NewBranch { from_label, .. } => {
                tr!("git.new_branch_from", from = from_label.as_str())
            }
            RefKindAsked::RenameBranch { old } => tr!("git.rename_branch_of", name = old.as_str()),
            RefKindAsked::NewTag { on_label, .. } => tr!("git.new_tag_on", on = on_label.as_str()),
        })
    }

    fn modal_height(&self) -> Option<f32> {
        self.request.as_ref().map(|r| height(&r.kind))
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(if self.is_tag() { "tag" } else { "git-branch" })
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], self.go_label())]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input().insert(&typed);
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text.lines().next().unwrap_or_default())
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

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let Some(kind) = self.request.as_ref().map(|r| r.kind.clone()) else { return };
        let pad = px(20.0);
        let layer = scene.push_layer(Some(b));
        let problem = self.problem();
        let style = |bad: bool| ds::field_style(&ds, ds.code_font(-0.5), bad);
        let field =
            Rect::new(b.min_x() + pad, b.min_y() + px(10.0), b.size.width - 2.0 * pad, px(36.0));
        let placeholder = if self.is_tag() { t("git.tag_name") } else { t("git.branch_name") };
        let bad = problem.is_some() && !self.name.text().is_empty();
        self.name.paint(layer, text, field, self.field == Field::Name, &placeholder, &style(bad));
        self.hits.push((field, Hit::Name));
        let small = ds.ui_font(-1.0);
        let mut y = field.max_y() + px(8.0);
        let (note, color) = match (&self.failed, &problem) {
            (Some(failed), _) => (failed.clone(), p.danger),
            (None, Some(why)) if !self.name.text().is_empty() => (why.clone(), p.danger),
            _ => (t("git.ref_note"), p.text_muted),
        };
        let shown = ds::ellipsize(text, &note, &small, b.size.width - 2.0 * pad);
        let shaped = text.shape_line(&shown, &small);
        draw_text(
            layer,
            &shaped,
            b.min_x() + pad,
            ds::optical_baseline(y, px(20.0), &small),
            color,
        );
        y += px(30.0);
        match kind {
            RefKindAsked::NewBranch { .. } => {
                // Checked out after creating it.
                let row = Rect::new(b.min_x() + pad, y, b.size.width - 2.0 * pad, px(28.0));
                let state = if self.checkout { ds::Check::On } else { ds::Check::Off };
                let mid = row.min_y() + row.size.height / 2.0;
                let r = ds::checkbox(layer, text, &ds, row.min_x(), mid, state, true);
                let label = text.shape_line(&t("git.checkout_after"), &ds.ui_font(-0.5));
                draw_text(
                    layer,
                    &label,
                    r.max_x() + px(8.0),
                    ds::optical_baseline(row.min_y(), row.size.height, &ds.ui_font(-0.5)),
                    p.text,
                );
                self.hits.push((row, Hit::Checkout));
            }
            RefKindAsked::NewTag { .. } => {
                // The message (an annotated tag).
                let field = Rect::new(b.min_x() + pad, y, b.size.width - 2.0 * pad, px(36.0));
                let focused = self.field == Field::Message;
                self.message.paint(
                    layer,
                    text,
                    field,
                    focused,
                    &t("git.tag_message"),
                    &style(false),
                );
                self.hits.push((field, Hit::Message));
            }
            RefKindAsked::RenameBranch { .. } => {}
        }
        // The buttons.
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(56.0) + (px(56.0) - bh) / 2.0;
        let go = self.go_label();
        let w = ds::primary_width(text, &ds, &[&go]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        let can = problem.is_none() && self.running.is_none();
        ds::primary_button(layer, text, &ds, g, &go, can, self.hover == Some(Hit::Go));
        self.hits.push((g, Hit::Go));
        let cancel = t("git.ref_cancel");
        let cw = ds::primary_width(text, &ds, &[&cancel]);
        let c = Rect::new(g.min_x() - px(10.0) - cw, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &cancel, true, self.hover == Some(Hit::Cancel));
        self.hits.push((c, Hit::Cancel));
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        match self.hit(p) {
            Some(Hit::Name) => {
                self.field = Field::Name;
                self.name.press(p.x, clicks);
            }
            Some(Hit::Message) => {
                self.field = Field::Message;
                self.message.press(p.x, clicks);
            }
            Some(Hit::Checkout) => self.checkout = !self.checkout,
            Some(Hit::Go) => {
                self.give();
            }
            Some(Hit::Cancel) => self.action = Some(ViewerAction::Close),
            None => {}
        }
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.input().extend_to(p.x);
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
            Hit::Name | Hit::Message => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => {
                self.give();
            }
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Tab if self.is_tag() => {
                self.field = if self.field == Field::Name { Field::Message } else { Field::Name };
            }
            ViewerKey::Left => self.input().move_left(reach.word, reach.extend),
            ViewerKey::Right => self.input().move_right(reach.word, reach.extend),
            ViewerKey::Home => self.input().home(reach.extend),
            ViewerKey::End => self.input().end(reach.extend),
            ViewerKey::Backspace => self.input().backspace(reach.word),
            ViewerKey::Delete => self.input().delete(),
            _ => return false,
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        let input = if self.field == Field::Name { &self.name } else { &self.message };
        let selected = input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "go" => self.give(),
            "select_all" => {
                self.input().select_all();
                true
            }
            "undo" => self.input().undo(),
            "redo" => self.input().redo(),
            "cut" => {
                let had = !self.input().selected_text().is_empty();
                if had {
                    self.input().insert("");
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

impl RefDialog {
    fn go_label(&self) -> String {
        match self.request.as_ref().map(|r| &r.kind) {
            Some(RefKindAsked::RenameBranch { .. }) => t("git.ref_rename"),
            _ => t("git.ref_create"),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/ref_dialog.rs"]
mod tests;
