//! The Git tool window's "Changes" tab: the working tree's changed files (what the button
//! counts), each with its state; a click opens the file, its "Compare" shows it beside its
//! last commit; "Commit…" in the header. The list is the project's (see [`crate::status`]:
//! read in the background, again as the files change); opening the tab reads it again.

use std::path::Path;
use std::sync::Arc;

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    AppRequest, ViewMenuEntry, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::draw_text;
use version_control::application::compare_with;
use version_control::{FileState, FileStatus, Repository, Revision};

use crate::files_tree::{FilesTree, TreeEvent};
use crate::parts::{Tune, Tuned};
use ui_components::changed_files::TreePart;
use ui_components::control::{Control, Controls};

/// The extension of the view (in the Git tool window).
pub const EXTENSION: &str = "view-git-changes";
/// The name it's found by in the Git window (the same in every language).
pub const KEY: &str = "git.changes";

/// What the view is opened with: the project (its changes), its repository and queries (the
/// files' icons).
#[derive(Clone)]
pub struct ChangesData {
    pub project: kernel::ProjectId,
    pub repo: Arc<dyn Repository>,
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
}

#[derive(Debug)]
pub(crate) struct ChangesProvider;

impl ViewerProvider for ChangesProvider {
    fn id(&self) -> &'static str {
        "git.changes"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ChangesList::default()))
    }
}

/// The rows of a file's menu (its index after the prefix).
const OPEN_FILE: &str = "file.open:";
const FILE_DIFF: &str = "file.diff:";
const COPY_PATH: &str = "file.copy-path:";
const SHOW_IN_PROJECT: &str = "file.show-in-project:";
const REVEAL_FILE: &str = "file.reveal:";

#[derive(Default)]
pub struct ChangesList {
    project: Option<kernel::ProjectId>,
    repo: Option<Arc<dyn Repository>>,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    files: Arc<Vec<FileStatus>>,
    /// The list's generation shown (0: not read yet).
    generation: u64,
    tree: FilesTree,
    /// What the tree opened, as a control ([`TreePart`]).
    opened: Option<TreeEvent>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
    /// "Commit…" (from the last paint), under the mouse.
    commit: Rect,
    commit_hover: bool,
    sizes: Option<workbench_model::LayoutSizes>,
    pending: Option<ViewerAction>,
    /// A comparison being read.
    opening: ui_components::deferred::Deferred,
    /// Made again from a session where no repository is found: its tab closes.
    gone: bool,
}

impl std::fmt::Debug for ChangesList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangesList").field("files", &self.files.len()).finish_non_exhaustive()
    }
}

impl ChangesList {
    /// File `i`, relative and on disk.
    fn relative(&self, i: usize) -> Option<&str> {
        Some(self.files.get(i)?.path.as_str())
    }

    fn path(&self, i: usize) -> Option<std::path::PathBuf> {
        Some(self.repo.as_ref()?.root().join(self.relative(i)?))
    }

    /// File `i` beside its last commit (a deleted one: what it was), read in the background.
    fn compare(&mut self, i: usize) -> Option<ViewerAction> {
        let (repo, path) = (self.repo.clone()?, self.path(i)?);
        self.opening.start("git-output", move || {
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            crate::DISK.read(current.len() as u64);
            let input = compare_with(repo.as_ref(), &path, &current, &Revision::Head, "HEAD")?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(ViewerAction::Compare(Box::new(input), format!("{name} ↔ HEAD")))
        });
        None
    }

    /// What opening file `i` does: its change beside its last commit, or, with nobody to
    /// show comparisons, the file.
    fn open(&mut self, i: usize) -> Option<ViewerAction> {
        if workbench_model::presence::compare() {
            return self.compare(i);
        }
        self.path(i).map(|path| ViewerAction::OpenFile { path, line: None, column: None })
    }

    /// The tree's files: the project's changes.
    fn refresh(&mut self) {
        let files =
            self.files.iter().map(|f| crate::files_tree::changed(&f.path, None, f.state)).collect();
        self.tree.set_files(files);
    }

    /// File `i`'s menu: opened, its change, its path copied, shown in the project view or
    /// the file manager.
    fn file_menu(&self, i: usize) -> Vec<ViewMenuEntry> {
        let deleted = self.files.get(i).is_some_and(|f| f.state == FileState::Deleted);
        let mut rows = Vec::new();
        if !deleted {
            rows.push(
                ViewMenuEntry::item(format!("{OPEN_FILE}{i}"), t("git.open_file"))
                    .with_icon("file"),
            );
        }
        if workbench_model::presence::compare() {
            rows.push(
                ViewMenuEntry::item(format!("{FILE_DIFF}{i}"), t("git.show_diff_working"))
                    .with_icon("compare"),
            );
        }
        rows.extend([
            ViewMenuEntry::Separator,
            ViewMenuEntry::item(format!("{COPY_PATH}{i}"), t("git.copy_path")).with_icon("link"),
            ViewMenuEntry::Separator,
        ]);
        if !deleted {
            rows.push(
                ViewMenuEntry::item(format!("{SHOW_IN_PROJECT}{i}"), t("git.show_in_project"))
                    .with_icon("crosshair"),
            );
            rows.push(
                ViewMenuEntry::item(format!("{REVEAL_FILE}{i}"), t("git.reveal_file"))
                    .with_icon("folder-open"),
            );
        }
        rows
    }
}

impl ChangesList {
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

    /// What a press or a key on the tree opened.
    fn take_opened(&mut self) -> Option<ViewerAction> {
        match self.opened.take()? {
            TreeEvent::Open(i) => self.open(i),
        }
    }
}

impl Viewer for ChangesList {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.tree.set_flat(sizes.get(crate::files_tree::LIST_MODE).is_some_and(|v| v > 0.5));
            self.sizes = Some(sizes.clone());
            return;
        }
        // Made again at a start: its project's repository found again.
        if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            let repo = restored.project.as_deref().and_then(git::GitRepository::discover);
            match (restored.id, repo) {
                (Some(project), Some(repo)) => {
                    let data = ChangesData {
                        project,
                        repo: Arc::new(repo),
                        queries: restored.queries.clone(),
                    };
                    self.set_data(&data);
                }
                _ => self.gone = true,
            }
            return;
        }
        let Some(data) = data.downcast_ref::<ChangesData>() else { return };
        self.project = Some(data.project);
        self.repo = Some(Arc::clone(&data.repo));
        self.queries.clone_from(&data.queries);
        if let Some(changes) = crate::status::changes(data.project) {
            changes.stale();
        }
        self.poll();
    }

    /// Opened on demand ("Show Changes"), no longer a fixed tab: closed like any.
    fn closable(&self) -> bool {
        true
    }

    /// Not made again at the next start (the Git window's fixed tabs are the Log and the
    /// Commit).
    fn restorable(&self) -> bool {
        false
    }

    fn panel_key(&self) -> Option<&'static str> {
        Some(KEY)
    }

    /// Made again where there's no repository any more: it goes.
    fn closed(&self) -> bool {
        self.gone
    }

    /// The project's list, when it was read again.
    fn poll(&mut self) -> bool {
        let opened = self.opening.poll();
        let ready = opened.is_some();
        if ready {
            self.pending = opened;
        }
        let Some(changes) = self.project.and_then(crate::status::changes) else { return ready };
        let (generation, files) = changes.files();
        let changed = generation != self.generation;
        if changed {
            (self.generation, self.files) = (generation, files);
            self.refresh();
        }
        changed || ready || self.opening.is_busy()
    }

    /// Anywhere over it, the wheel scrolls the part that scrolls (the tree).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|_, parts| parts.iter_mut().any(|part| part.scroll(dx, dy)))
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        if self.commit.contains(p) {
            return Some(ViewerAction::RunCommand(crate::COMMIT.0.to_owned()));
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
        // The app's edit commands, for the search.
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        let file = |prefix: &str| id.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok());
        self.pending = if let Some(i) = file(OPEN_FILE) {
            self.path(i).map(|path| ViewerAction::OpenFile { path, line: None, column: None })
        } else if let Some(i) = file(FILE_DIFF) {
            self.compare(i)
        } else if let Some(i) = file(COPY_PATH) {
            self.relative(i).map(|p| ViewerAction::Request(AppRequest::CopyText(p.to_owned())))
        } else if let Some(i) = file(SHOW_IN_PROJECT) {
            self.path(i).map(|path| ViewerAction::Request(AppRequest::ShowFolder(path)))
        } else if let Some(i) = file(REVEAL_FILE) {
            self.path(i).map(|path| ViewerAction::Request(AppRequest::Reveal(path)))
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
        let over = self.commit.contains(p);
        let changed = over != self.commit_hover;
        self.commit_hover = over;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    /// The files' scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.controls.tooltip_at(p)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.opening.set_waker(&ctx.waker);
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        // The header: the title (or that it's being read), "Commit…".
        let header_h = px(44.0);
        let strong = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let title =
            if self.generation == 0 { t("git.changes_reading") } else { t("git.changes_tab") };
        let shaped = ctx.text.shape_line(&title, &strong);
        let baseline = ds::optical_baseline(b.min_y(), header_h, &strong);
        draw_text(layer, &shaped, b.min_x() + px(16.0), baseline, p.text);
        self.commit = Rect::default();
        if !self.files.is_empty() {
            // A header's button, as the log's; in the accent: the main action.
            let label = t("command.git_commit");
            let width = ds::toolbar_button_width(ctx.text, &ds, &label);
            let h = px(ds::TOOLBAR_BUTTON_H);
            let top = b.min_y() + ((header_h - h) / 2.0).round();
            let button = Rect::new(b.max_x() - px(16.0) - width, top, width, h);
            let spec = ds::ToolbarButton {
                label: &label,
                icon: "git-commit",
                accent: true,
                hovered: self.commit_hover,
            };
            ds::toolbar_button(layer, ctx.text, ctx.icons, &ds, button, spec);
            self.commit = button;
        }
        layer.quad(Quad::filled(
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, ds.line()),
            p.card_border,
        ));
        let list =
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, b.size.height - header_h);
        if self.files.is_empty() {
            if self.generation > 0 {
                ds::empty_state(layer, ctx.text, &ds, list, &t("git.changes_none"), None);
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
#[path = "../tests/unit/changes_view.rs"]
mod tests;
