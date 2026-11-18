//! Files and folders dropped from the system's file manager (Finder, Explorer): what a drop
//! opens — each folder as a project (the first in this window when it's empty, as "Open
//! Project…" does; the others in windows of their own), the files as tabs here, in the
//! editor group under the drop — and the overlay while they're dragged over the window:
//! the window dimmed, a card saying what the drop would open, the group the files would go
//! to outlined. A modal open refuses the drop (nothing behind it gets input).
//!
//! The overlay floats over the stack ([`OverlaySlot::floats`]): it never has the keyboard
//! and takes nothing (while the system drags, the window gets no other input anyway).

use render_api::Scene;
use text_layout::Font;
use ui_workbench::ds;

use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;

/// The files a drop opens at most (the rest are counted, and said).
pub const MAX_FILES: usize = 50;
/// The folders a drop opens as projects at most (each may be a window).
pub const MAX_PROJECTS: usize = 10;
/// The items asked of the disk at most (a drop of thousands): the rest are skipped.
const MAX_LOOKED_AT: usize = 1000;

/// What a dropped item is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    File,
    Folder,
}

/// The items of a drop, sorted: what exists, within the limits, in the order dropped.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DropPlan {
    /// The folders (their links resolved), each a project.
    pub folders: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    /// Over the limits: not opened.
    pub skipped: usize,
    /// Gone from the disk since (or never there): ignored.
    pub missing: usize,
}

/// Sorts `paths` by what `kind_of` says they are (`None`: not there).
pub fn plan_drop(paths: &[PathBuf], kind_of: impl Fn(&Path) -> Option<Dropped>) -> DropPlan {
    let mut plan = DropPlan::default();
    for (i, path) in paths.iter().enumerate() {
        let full = plan.files.len() >= MAX_FILES && plan.folders.len() >= MAX_PROJECTS;
        if i >= MAX_LOOKED_AT || full {
            plan.skipped += paths.len() - i;
            break;
        }
        match kind_of(path) {
            Some(Dropped::Folder) if plan.folders.len() < MAX_PROJECTS => {
                plan.folders.push(path.clone());
            }
            Some(Dropped::File) if plan.files.len() < MAX_FILES => plan.files.push(path.clone()),
            Some(_) => plan.skipped += 1,
            None => plan.missing += 1,
        }
    }
    plan
}

/// What a drop does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// A modal is open: nothing behind it takes input, drops neither.
    Refused,
    /// None of the items is on disk anymore.
    Nothing,
    Open(DropOpen),
}

/// What a drop opens.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DropOpen {
    /// The folder opened as this window's project, as "Open Project…" opens it (straight in
    /// an empty window; it asks where otherwise).
    pub here: Option<PathBuf>,
    /// The folders opened in windows of their own (an open one's window to the front).
    pub elsewhere: Vec<PathBuf>,
    /// The files opened as tabs in this window, the last one active.
    pub files: Vec<PathBuf>,
    pub skipped: usize,
}

/// What `plan` does in a window with a modal open or not, with content (a project or
/// tabs) or not. A single folder dropped alone asks like "Open Project…" in a window with
/// content; several, or with files, go to windows of their own then (choosing "this
/// window" would close the files just dropped).
pub fn drop_action(plan: DropPlan, modal_open: bool, has_content: bool) -> DropAction {
    if modal_open {
        return DropAction::Refused;
    }
    if plan.folders.is_empty() && plan.files.is_empty() {
        return DropAction::Nothing;
    }
    let DropPlan { folders, files, skipped, .. } = plan;
    let alone = folders.len() == 1 && files.is_empty();
    let mut folders = folders.into_iter();
    let here = if !has_content || alone { folders.next() } else { None };
    DropAction::Open(DropOpen { here, elsewhere: folders.collect(), files, skipped })
}

/// What the overlay says of a drop: its icon, its title, the lines under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropText {
    pub icon: &'static str,
    pub title: String,
    pub lines: Vec<String>,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The overlay's text for `action`.
pub fn drop_text(action: &DropAction) -> DropText {
    match action {
        DropAction::Refused => DropText {
            icon: "lock",
            title: i18n::t("drop.refused_title"),
            lines: vec![i18n::t("drop.refused_detail")],
        },
        DropAction::Nothing => DropText {
            icon: "circle-alert",
            title: i18n::t("drop.nothing_title"),
            lines: vec![i18n::t("drop.nothing_detail")],
        },
        DropAction::Open(open) => {
            let mut parts = Vec::new();
            let folders: Vec<&PathBuf> = open.here.iter().chain(&open.elsewhere).collect();
            match folders.as_slice() {
                [] => {}
                [only] if open.here.is_some() => {
                    parts.push(i18n::tr!("drop.project", name = name_of(only)));
                }
                [only] => parts.push(i18n::tr!("drop.project_new_window", name = name_of(only))),
                many => parts.push(i18n::tr!("drop.projects", count = many.len())),
            }
            match open.files.as_slice() {
                [] => {}
                [only] => parts.push(i18n::tr!("drop.file", name = name_of(only))),
                many => parts.push(i18n::tr!("drop.files", count = many.len())),
            }
            let mut lines = vec![parts.join(" · ")];
            if open.skipped > 0 {
                lines.push(i18n::tr!(
                    "drop.over_limit",
                    count = open.skipped,
                    files = MAX_FILES,
                    folders = MAX_PROJECTS
                ));
            }
            let icon = if open.files.is_empty() { "folder-input" } else { "download" };
            DropText { icon, title: i18n::t("drop.title"), lines }
        }
    }
}

/// The toast after a drop over the limits.
pub fn skipped_message(skipped: usize) -> String {
    i18n::tr!("drop.skipped", count = skipped, files = MAX_FILES, folders = MAX_PROJECTS)
}

/// A drag from the file manager over the window.
#[derive(Debug, Clone)]
pub(super) struct FileDropHover {
    /// Its items, sorted once as it entered.
    plan: DropPlan,
    /// The pointer (physical pixels).
    at: Point,
}

impl Workbench {
    /// `paths` sorted through the disk's port (folders' links resolved, as a project's root
    /// is).
    pub fn plan_drop(&self, paths: &[PathBuf]) -> DropPlan {
        let mut plan = plan_drop(paths, |p| match self.ports.disk.kind(p)? {
            NodeKind::Directory => Some(Dropped::Folder),
            NodeKind::File => Some(Dropped::File),
        });
        let folders = std::mem::take(&mut plan.folders);
        for folder in folders {
            match self.ports.disk.canonical(&folder) {
                Ok(real) => plan.folders.push(simplify_path(real)),
                Err(_) => plan.missing += 1,
            }
        }
        plan
    }

    /// What `plan` does in this window now.
    pub fn drop_action(&self, plan: DropPlan) -> DropAction {
        drop_action(plan, self.modal_open(), self.has_content())
    }

    /// Items from the file manager dragged over the window, the pointer `at`.
    pub fn drag_files_over(&mut self, paths: &[PathBuf], at: Point) {
        let plan = self.plan_drop(paths);
        tracing::debug!(items = paths.len(), "files dragged over the window");
        self.file_drop = Some(FileDropHover { plan, at });
    }

    /// The pointer moved during such a drag: `true` if the group the files would go to
    /// changed (a redraw).
    pub fn move_file_drag(&mut self, at: Point) -> bool {
        let before = self.file_drop.as_ref().map(|h| self.drop_group(h.at));
        let Some(hover) = &mut self.file_drop else { return false };
        hover.at = at;
        before != Some(self.drop_group(at))
    }

    /// The drag ended (dropped or gone): `true` if there was one (a redraw).
    pub fn end_file_drag(&mut self) -> bool {
        self.file_drop.take().is_some()
    }

    pub fn file_drag_active(&self) -> bool {
        self.file_drop.is_some()
    }

    /// The editor group dropped files go to: the one under `at`, else the current one.
    pub(super) fn drop_group(&self, at: Point) -> usize {
        match self.editors.groups.as_slice() {
            [_] => self.editors.current,
            groups => {
                groups.iter().position(|g| g.bounds.contains(at)).unwrap_or(self.editors.current)
            }
        }
    }

    /// The group under `at` made current, before dropped files open in it.
    pub fn focus_drop_group(&mut self, at: Point) {
        let g = self.drop_group(at);
        self.focus_group(g);
    }

    /// What the drag over the window would do now, the group outlined, and the card.
    #[cfg(test)]
    pub(super) fn file_drop_shown(&self) -> Option<(DropAction, Option<usize>)> {
        let hover = self.file_drop.as_ref()?;
        let action = self.drop_action(hover.plan.clone());
        let group = match &action {
            DropAction::Open(open) if !open.files.is_empty() => Some(self.drop_group(hover.at)),
            _ => None,
        };
        Some((action, group))
    }

    fn paint_file_drop(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let Some(hover) = &self.file_drop else { return };
        let action = self.drop_action(hover.plan.clone());
        let ctx = ds::Ctx::new(&self.palette, self.scale);
        let p = &self.palette;
        let window = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        let layer = scene.push_layer(None);
        ds::backdrop(layer, &ctx, window);
        // Files: the group they'd open in, outlined; the card over it.
        let mut area = window;
        if let DropAction::Open(open) = &action
            && !open.files.is_empty()
            && let Some(group) = self.editors.groups.get(self.drop_group(hover.at))
            && group.bounds.size.width > 0.0
            && group.bounds.size.height > 0.0
        {
            area = group.bounds;
            super::splits::paint_drop_zone(layer, area, self.scale, p);
        }
        let shown = drop_text(&action);
        let title_font = ctx.ui_font(1.5).with_weight(Font::SEMIBOLD);
        let body_font = ctx.ui_font(0.0);
        let title = text.shape_line(&shown.title, &title_font);
        let lines: Vec<_> = shown.lines.iter().map(|l| text.shape_line(l, &body_font)).collect();
        let pad = ctx.px(24.0);
        let icon = ctx.px(28.0);
        let gap = ctx.px(12.0);
        let title_h = ctx.px(24.0);
        let line_h = ctx.px(20.0);
        let widest = lines.iter().map(|l| l.width).fold(title.width, f32::max);
        let w = (widest + 2.0 * pad).max(ctx.px(280.0)).min(window.size.width - ctx.px(32.0));
        let h = pad + icon + gap + title_h + line_h * lines.len() as f32 + pad;
        let card = Rect::new(
            (area.min_x() + (area.size.width - w) / 2.0).round(),
            (area.min_y() + (area.size.height - h) / 2.0).round(),
            w,
            h,
        );
        ds::surface(layer, &ctx, card, ds::Elevation::High, 12.0);
        let color = match action {
            DropAction::Open(_) => p.accent,
            DropAction::Refused | DropAction::Nothing => p.text_muted,
        };
        if let Some(id) = self.icons.id(shown.icon) {
            let at = Rect::new(card.min_x() + (w - icon) / 2.0, card.min_y() + pad, icon, icon);
            draw_icon(layer, &self.icons, id, at, color);
        }
        let mut y = card.min_y() + pad + icon + gap;
        let centered = |width: f32| (card.min_x() + (w - width) / 2.0).round();
        draw_text(
            layer,
            &title,
            centered(title.width),
            ds::optical_baseline(y, title_h, &title_font),
            p.text,
        );
        y += title_h;
        for line in &lines {
            draw_text(
                layer,
                line,
                centered(line.width),
                ds::optical_baseline(y, line_h, &body_font),
                p.text_muted,
            );
            y += line_h;
        }
    }
}

/// The drop overlay, over the whole stack (see the module's doc).
pub(super) struct FileDropSlot;

impl OverlaySlot for FileDropSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.file_drop.is_some()
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    // Files dragged over the window: the whole of it.
    fn area(&self, _wb: &Workbench) -> Option<kernel::Rect> {
        Some(kernel::Rect::new(0.0, 0.0, f32::MAX, f32::MAX))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_file_drop(text, scene);
    }

    fn animating(&self, _wb: &Workbench) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/file_drop.rs"]
mod tests;
