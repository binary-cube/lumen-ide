//! The Git tool window's "Conflicts" tab: what's in progress (`Rebase · feature · 3/7`) with
//! its steps (Continue, Skip, Abort: the module's commands), and the conflicted files. A
//! click resolves one in three parts (ours, the result, theirs); its menu takes either
//! side's whole version, marks it resolved as it is, opens it. The list is the project's
//! changes (see [`crate::status`]); the files' actions run in the background, the list read
//! again after.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    AppRequest, ViewMenuEntry, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::draw_text;
use version_control::{
    FileState, Operation, OperationKind, OperationStep, Repository, Side, VcsError,
};

use crate::files_tree::{FilesTree, TreeEvent};
use crate::parts::{Tune, Tuned};
use ui_components::changed_files::TreePart;
use ui_components::control::{Control, Controls};

pub(crate) use crate::operation::operation_text;

/// The extension of the view (in the Git tool window).
pub const EXTENSION: &str = "view-git-conflicts";
/// The name it's found by in the Git window (the same in every language).
pub const KEY: &str = "git.conflicts";

/// What the view is opened with: the project (its changes), its repository and queries (the
/// files' icons).
#[derive(Clone)]
pub struct ConflictsData {
    pub project: kernel::ProjectId,
    pub repo: Arc<dyn Repository>,
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
}

#[derive(Debug)]
pub(crate) struct ConflictsProvider;

impl ViewerProvider for ConflictsProvider {
    fn id(&self) -> &'static str {
        "git.conflicts"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ConflictsList::default()))
    }
}

/// The rows of a file's menu (its index after the prefix).
const RESOLVE: &str = "conflict.resolve:";
const TAKE_OURS: &str = "conflict.ours:";
const TAKE_THEIRS: &str = "conflict.theirs:";
const MARK_RESOLVED: &str = "conflict.resolved:";
const OPEN_FILE: &str = "conflict.open:";
const COPY_PATH: &str = "conflict.copy-path:";
const SHOW_IN_PROJECT: &str = "conflict.show-in-project:";

/// The header's steps, left to right: the command doing each, its label's key, its icon.
const STEPS: [(OperationStep, command_model::CommandId, &str, &str); 3] = [
    (OperationStep::Abort, crate::ABORT_OPERATION, "git.step_abort", "rotate-ccw"),
    (OperationStep::Skip, crate::SKIP_OPERATION, "git.step_skip", "arrow-right-to-line"),
    (OperationStep::Continue, crate::CONTINUE_OPERATION, "git.step_continue", "play"),
];

#[derive(Default)]
pub struct ConflictsList {
    project: Option<kernel::ProjectId>,
    repo: Option<Arc<dyn Repository>>,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    /// The conflicted files (relative, sorted) and the operation, as last read.
    files: Vec<String>,
    operation: Option<Operation>,
    /// The list's generation shown (0: not read yet).
    generation: u64,
    tree: FilesTree,
    /// What the tree opened, as a control ([`TreePart`]).
    opened: Option<TreeEvent>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
    /// The header's step buttons (from the last paint) and the one under the mouse.
    buttons: Vec<(Rect, command_model::CommandId)>,
    hovered: Option<usize>,
    /// A file action running (its outcome comes back here).
    running: Option<Receiver<Result<(), VcsError>>>,
    /// The last action's failure, under the header.
    failure: Option<String>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    sizes: Option<workbench_model::LayoutSizes>,
    pending: Option<ViewerAction>,
    /// A file's versions being read, to resolve it.
    opening: ui_components::deferred::Deferred,
    /// Made again where there's no repository: its tab closes.
    gone: bool,
}

impl std::fmt::Debug for ConflictsList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConflictsList").field("files", &self.files.len()).finish_non_exhaustive()
    }
}

impl ConflictsList {
    fn path(&self, i: usize) -> Option<std::path::PathBuf> {
        Some(self.repo.as_ref()?.root().join(self.files.get(i)?))
    }

    /// File `i` resolved in three parts (ours, the result, theirs), marked resolved once
    /// its result is saved. Deleted on one side (nothing to merge line by line): opened at
    /// its first conflict instead.
    fn resolve(&mut self, i: usize) -> Option<ViewerAction> {
        let (repo, relative, path) =
            (self.repo.clone()?, self.files.get(i)?.clone(), self.path(i)?);
        let (project, sides) = (self.project, side_names(self.operation.as_ref()));
        // Read in the background: git's versions and the file may be large.
        self.opening.start("git-conflicts", move || {
            let versions =
                version_control::application::conflict_versions(repo.as_ref(), &relative);
            let merging = workbench_model::presence::compare();
            let (Some(ours), Some(theirs)) = (versions.ours.filter(|_| merging), versions.theirs)
            else {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                crate::DISK.read(text.len() as u64);
                let line =
                    version_control::Merged::parse(&text).hunk_lines(0).map_or(0, |l| l.start);
                return Some(ViewerAction::OpenAt { path, line: u32::try_from(line).unwrap_or(0) });
            };
            if let Some(changes) = project.and_then(crate::status::changes) {
                changes.resolve_on_save(path.clone(), Arc::clone(&repo));
            }
            let name = kernel::paths::file_name(&path);
            let input = diff::MergeInput {
                result: path,
                base: versions.base.unwrap_or_default(),
                ours,
                theirs,
                labels: sides.map(|side| format!("{name} · {side}")),
            };
            let title = tr!("git.merge_title", name = name.as_str());
            Some(ViewerAction::OpenView(ui_components::OpenView::new(
                title,
                diff::COMPARE_VIEW,
                input,
            )))
        });
        None
    }

    /// `job` run on file `i` in the background (one at a time); the list read again after.
    fn run(&mut self, i: usize, job: fn(&dyn Repository, &[String]) -> Result<(), VcsError>) {
        let (Some(repo), Some(path)) = (self.repo.clone(), self.files.get(i).cloned()) else {
            return;
        };
        if self.running.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (waker, project) = (self.waker.clone(), self.project);
        kernel::work::run("git-conflicts", move || {
            let _ = tx.send(job(repo.as_ref(), &[path]));
            if let Some(changes) = project.and_then(crate::status::changes) {
                changes.stale();
            }
            if let Some(wake) = waker {
                wake();
            }
        });
        self.running = Some(rx);
        self.failure = None;
    }

    /// File `i`'s menu.
    fn file_menu(&self, i: usize) -> Vec<ViewMenuEntry> {
        let (ours, theirs) = side_labels(self.operation.as_ref());
        let row = |prefix: &str, label: String, icon: &'static str| {
            ViewMenuEntry::item(format!("{prefix}{i}"), label).with_icon(icon)
        };
        vec![
            row(RESOLVE, t("git.conflict_resolve"), "compare"),
            ViewMenuEntry::Separator,
            row(TAKE_OURS, ours, "arrow-left"),
            row(TAKE_THEIRS, theirs, "arrow-right"),
            row(MARK_RESOLVED, t("git.conflict_mark_resolved"), "check"),
            ViewMenuEntry::Separator,
            row(OPEN_FILE, t("git.open_file"), "file"),
            row(COPY_PATH, t("git.copy_path"), "link"),
            row(SHOW_IN_PROJECT, t("git.show_in_project"), "crosshair"),
        ]
    }

    /// The header's title: what's in progress and how many conflicts are left.
    fn title(&self) -> String {
        if self.generation == 0 {
            return t("git.conflicts_reading");
        }
        let count = tr!("git.conflicts_count", count = self.files.len());
        match &self.operation {
            Some(operation) => format!("{} — {count}", operation_text(operation)),
            None => count,
        }
    }
}

/// What each side is called in the three-part merge (ours, theirs): during a rebase, what
/// it's rebased onto and the commit replayed.
pub(crate) fn side_names(operation: Option<&Operation>) -> [String; 2] {
    if operation.is_some_and(|op| op.kind == OperationKind::Rebase) {
        [t("git.side_upstream"), t("git.side_replayed")]
    } else {
        [t("git.side_ours"), t("git.side_theirs")]
    }
}

/// What taking each side is called: during a rebase, "ours" is what's rebased onto and
/// "theirs" the commit replayed (git's words turned around), so it's said plainly.
pub(crate) fn side_labels(operation: Option<&Operation>) -> (String, String) {
    if operation.is_some_and(|op| op.kind == OperationKind::Rebase) {
        (t("git.take_upstream"), t("git.take_replayed"))
    } else {
        (t("git.take_ours"), t("git.take_theirs"))
    }
}

impl ConflictsList {
    /// Its parts used as controls, with their state of use: the files tree, which has the
    /// keys and the text whatever else is pressed (given back the focus when it lost it),
    /// and answers Tab itself.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut tree = TreePart { tree: &mut self.tree, event: &mut self.opened };
        let tune = Tune { keeps_tab: true, ..Tune::default() };
        let mut tree = Tuned { part: &mut tree, tune };
        let mut parts: [&mut dyn Control; 1] = [&mut tree];
        if self.controls.focused().is_none() {
            self.controls.focus(&mut parts, Some(0));
        }
        f(&mut self.controls, &mut parts)
    }

    /// What a press or a key on the tree opened: its file resolved.
    fn take_opened(&mut self) -> Option<ViewerAction> {
        match self.opened.take()? {
            TreeEvent::Open(i) => self.resolve(i),
        }
    }
}

impl Viewer for ConflictsList {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.tree.set_flat(sizes.get(crate::files_tree::LIST_MODE).is_some_and(|v| v > 0.5));
            self.sizes = Some(sizes.clone());
            return;
        }
        let Some(data) = data.downcast_ref::<ConflictsData>() else { return };
        self.project = Some(data.project);
        self.repo = Some(Arc::clone(&data.repo));
        self.queries.clone_from(&data.queries);
        if let Some(changes) = crate::status::changes(data.project) {
            changes.stale();
        }
        self.poll();
    }

    fn panel_key(&self) -> Option<&'static str> {
        Some(KEY)
    }

    fn closed(&self) -> bool {
        self.gone
    }

    /// The project's conflicts when they were read again, and a file action's outcome.
    fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(action) = self.opening.poll() {
            self.pending = Some(action);
            changed = true;
        }
        if let Some(rx) = &self.running
            && let Ok(result) = rx.try_recv()
        {
            self.running = None;
            self.failure = result.err().map(|e| crate::error_message(&e));
            changed = true;
        }
        let Some(changes) = self.project.and_then(crate::status::changes) else {
            return changed;
        };
        let (generation, _) = changes.files();
        if generation != self.generation {
            self.generation = generation;
            self.files = changes.conflicted();
            self.operation = changes.operation();
            let files = self
                .files
                .iter()
                .map(|path| crate::files_tree::changed(path, None, FileState::Conflicted))
                .collect();
            self.tree.set_files(files);
            changed = true;
        }
        changed || self.running.is_some() || self.opening.is_busy()
    }

    /// Anywhere over it, the wheel scrolls the part that scrolls (the tree).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|_, parts| parts.iter_mut().any(|part| part.scroll(dx, dy)))
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        if let Some((_, command)) = self.buttons.iter().find(|(r, _)| r.contains(p)) {
            return Some(ViewerAction::RunCommand(command.0.to_owned()));
        }
        let flat = self.tree.flat;
        self.with_controls(|c, parts| c.press(parts, p, clicks))?;
        if self.tree.flat != flat
            && let Some(sizes) = &self.sizes
        {
            sizes.set(crate::files_tree::LIST_MODE, if self.tree.flat { 1.0 } else { 0.0 });
        }
        self.take_opened()
    }

    fn wants_text(&self) -> bool {
        self.tree.searching
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, ui_components::KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        if self.with_controls(|c, parts| c.key(parts, key, reach)).is_none() {
            return false;
        }
        if self.opened.is_some() {
            self.pending = self.take_opened();
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let i = self.tree.file_at(p)?;
        Some(self.file_menu(i))
    }

    fn paste(&mut self, text: &str) -> bool {
        let line = text.lines().next().unwrap_or_default();
        self.with_controls(|c, parts| c.paste(parts, line)).is_some()
    }

    fn selected_text(&self) -> Option<String> {
        self.tree.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        let file = |prefix: &str| id.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok());
        if let Some(i) = file(TAKE_OURS) {
            self.run(i, |repo, paths| repo.take_side(paths, Side::Ours));
            return true;
        }
        if let Some(i) = file(TAKE_THEIRS) {
            self.run(i, |repo, paths| repo.take_side(paths, Side::Theirs));
            return true;
        }
        if let Some(i) = file(MARK_RESOLVED) {
            self.run(i, |repo, paths| repo.mark_resolved(paths));
            return true;
        }
        self.pending = if let Some(i) = file(RESOLVE) {
            self.resolve(i)
        } else if let Some(i) = file(OPEN_FILE) {
            self.path(i).map(|path| ViewerAction::OpenFile { path, line: None, column: None })
        } else if let Some(i) = file(COPY_PATH) {
            self.files.get(i).map(|p| ViewerAction::Request(AppRequest::CopyText(p.clone())))
        } else if let Some(i) = file(SHOW_IN_PROJECT) {
            self.path(i).map(|path| ViewerAction::Request(AppRequest::ShowFolder(path)))
        } else {
            return false;
        };
        true
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let over = self.buttons.iter().position(|(r, _)| r.contains(p));
        let changed = over != self.hovered;
        self.hovered = over;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.controls.tooltip_at(p)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.opening.set_waker(&ctx.waker);
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        // The header: what's in progress, its steps on the right (Continue in the accent once
        // nothing is left to resolve).
        let header_h = px(44.0);
        let strong = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = ctx.text.shape_line(&self.title(), &strong);
        let baseline = ds::optical_baseline(b.min_y(), header_h, &strong);
        draw_text(layer, &shaped, b.min_x() + px(16.0), baseline, p.text);
        self.buttons.clear();
        if let Some(operation) = &self.operation {
            let h = px(ds::TOOLBAR_BUTTON_H);
            let top = b.min_y() + ((header_h - h) / 2.0).round();
            let mut right = b.max_x() - px(16.0);
            for (step, command, key, icon) in STEPS.iter().rev() {
                if !operation.kind.allows(*step) {
                    continue;
                }
                let label = t(key);
                let width = ds::toolbar_button_width(ctx.text, &ds, &label);
                let button = Rect::new(right - width, top, width, h);
                let spec = ds::ToolbarButton {
                    label: &label,
                    icon,
                    accent: *step == OperationStep::Continue && self.files.is_empty(),
                    hovered: self.hovered == Some(self.buttons.len()),
                };
                ds::toolbar_button(layer, ctx.text, ctx.icons, &ds, button, spec);
                self.buttons.push((button, *command));
                right -= width + px(8.0);
            }
        }
        let mut top = b.min_y() + header_h;
        layer.quad(Quad::filled(Rect::new(b.min_x(), top, b.size.width, ds.line()), p.card_border));
        // The last action's failure, on its own line.
        if let Some(failure) = &self.failure {
            let line_h = px(28.0);
            let font = ds.ui_font(0.0);
            let shaped = ctx.text.shape_line(failure, &font);
            let baseline = ds::optical_baseline(top, line_h, &font);
            draw_text(layer, &shaped, b.min_x() + px(16.0), baseline, p.danger);
            top += line_h;
        }
        let list = Rect::new(b.min_x(), top, b.size.width, (b.max_y() - top).max(0.0));
        if self.files.is_empty() {
            if self.generation > 0 {
                let message = if self.operation.is_some() {
                    t("git.conflicts_none_continue")
                } else {
                    t("git.conflicts_none")
                };
                ds::empty_state(layer, ctx.text, &ds, list, &message, None);
            }
            self.with_controls(|c, parts| c.settle(parts));
            return;
        }
        let queries = self.queries.clone();
        self.tree.paint(
            ctx.scene,
            ctx.text,
            ctx.icons,
            &ds,
            (list, ctx.focused),
            queries.as_deref(),
        );
        self.with_controls(|c, parts| c.settle(parts));
    }
}

#[cfg(test)]
#[path = "../tests/unit/conflicts_view.rs"]
mod tests;
