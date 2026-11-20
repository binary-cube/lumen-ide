//! The branch list under the top bar's branch (or under the bar's middle, from the Git
//! menu): a search that has the keyboard as it opens (⌘A, ×, copy, cut, paste, undo as in
//! any field), "New Branch…" first, then the branches last checked out, the local ones and
//! the remote ones no local branch tracks yet — each with how far it is from what it
//! tracks, the current one ticked, the keys at the bottom. Choosing one switches to it as a
//! double click does ([`crate::switch`]).
//!
//! What it lists is read on a worker before it opens; a popover of the module's own
//! (`host.open_overlay`), closed by Esc, a choice or a click outside.

use std::path::Path;
use std::sync::Arc;

use i18n::{t, tr};
use kernel::{Point, Rect};
use ui_components::control::{Control, Controls};
use ui_components::ds::Ctx;
use ui_components::picker::{Picker, PickerEvent, PickerItem};
use ui_components::{
    KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use version_control::{RefKind, Repository, SyncState};
use workbench_model::{Anchor, CommandHandler, Completion, WorkbenchHost};

use crate::switch::Wanted;

/// The popover's virtual extension.
pub const EXTENSION: &str = "view-git-branches";

/// The branches last checked out it lists at most.
const RECENT: usize = 5;

/// A row: New Branch…, or a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    NewBranch,
    Branch { wanted: Wanted, current: bool },
}

/// What it opens with.
#[derive(Debug, Clone)]
pub struct BranchList {
    /// Under what (physical pixels); `None`: under the bar's middle.
    pub anchor: Option<Rect>,
    pub rows: Vec<Row>,
    pub items: Vec<PickerItem>,
}

/// How far `state` is from what it tracks (`↑2 ↓1 · origin/main`).
fn sync_detail(state: Option<&SyncState>) -> String {
    let Some(state) = state else { return String::new() };
    let mut parts = Vec::new();
    if state.ahead > 0 {
        parts.push(format!("↑{}", state.ahead));
    }
    if state.behind > 0 {
        parts.push(format!("↓{}", state.behind));
    }
    parts.extend(state.upstream.clone());
    parts.join(" · ")
}

/// The rows and their items: New Branch…, the recent branches, the other local ones, the
/// remote ones no local branch tracks.
pub(crate) fn list(repo: &dyn Repository) -> (Vec<Row>, Vec<PickerItem>) {
    let current = repo.branch().filter(|_| !repo.detached());
    let refs = repo.refs();
    let states = repo.branch_states();
    let state = |name: &str| states.iter().find(|s| s.branch == name);
    let locals: Vec<String> =
        refs.iter().filter(|r| r.kind == RefKind::Branch).map(|r| r.name.clone()).collect();
    let recent: Vec<String> = repo.recent_branches(RECENT);
    let mut rows = vec![Row::NewBranch];
    let mut items = vec![PickerItem {
        label: t("git.new_branch_row"),
        detail: t("git.new_branch_row_detail"),
        icon: Some("plus"),
        ..PickerItem::default()
    }];
    let mut local = |name: &str, detail: String| {
        rows.push(Row::Branch {
            wanted: Wanted::Branch(name.to_owned()),
            current: current.as_deref() == Some(name),
        });
        items.push(PickerItem {
            label: name.to_owned(),
            detail,
            icon: Some("git-branch"),
            ..PickerItem::default()
        });
    };
    // The current branch first, then the recent ones, then the others by name.
    if let Some(name) = &current {
        local(name, sync_detail(state(name)));
    }
    for name in recent.iter().filter(|r| Some(*r) != current.as_ref()) {
        let mut detail = t("git.branch_recent");
        let sync = sync_detail(state(name));
        if !sync.is_empty() {
            detail = format!("{detail} · {sync}");
        }
        local(name, detail);
    }
    for name in locals.iter().filter(|n| Some(*n) != current.as_ref() && !recent.contains(n)) {
        local(name, sync_detail(state(name)));
    }
    let tracked: Vec<&str> = states.iter().filter_map(|s| s.upstream.as_deref()).collect();
    for remote in refs.iter().filter(|r| r.kind == RefKind::Remote) {
        // `origin/HEAD` points at one of them; a tracked one is reached through its local.
        if remote.name.ends_with("/HEAD") || tracked.contains(&remote.name.as_str()) {
            continue;
        }
        rows.push(Row::Branch { wanted: Wanted::Remote(remote.name.clone()), current: false });
        items.push(PickerItem {
            label: remote.name.clone(),
            detail: t("git.branch_remote"),
            icon: Some("globe"),
            ..PickerItem::default()
        });
    }
    (rows, items)
}

/// Opens the list (read on a worker first) under the top bar's branch, else the bar's middle.
pub(crate) fn open(host: &mut dyn WorkbenchHost) -> bool {
    let Some(repo) = host.repository() else { return false };
    let anchor = host.anchor(Anchor::TopBarLabel);
    host.spawn(Box::new(move || {
        let (rows, items) = list(repo.as_ref());
        Box::new(move |host: &mut dyn WorkbenchHost| {
            host.open_overlay(EXTENSION, &BranchList { anchor, rows, items }, true);
        }) as Completion
    }));
    true
}

/// The Git menu's Switch Branch… (and the top bar's branch).
pub(crate) struct SwitchBranch;

impl CommandHandler for SwitchBranch {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.repository().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        open(host)
    }
}

#[derive(Debug)]
pub(crate) struct BranchPickerProvider;

impl ViewerProvider for BranchPickerProvider {
    fn id(&self) -> &'static str {
        "git.branches"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(BranchPicker::default()))
    }
}

#[derive(Default)]
pub struct BranchPicker {
    list: Option<Arc<BranchList>>,
    picker: Picker,
    action: Option<ViewerAction>,
    done: bool,
    /// The mouse is over the list (the wheel scrolls it, not what's under it).
    inside: bool,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for BranchPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BranchPicker").field("done", &self.done).finish_non_exhaustive()
    }
}

impl BranchPicker {
    /// What the picker said: a row chosen (switched to, or New Branch…), or closed.
    pub(crate) fn picked(&mut self, event: Option<PickerEvent>) {
        match event {
            Some(PickerEvent::Chosen(i)) => {
                self.done = true;
                let row = self.list.as_ref().and_then(|l| l.rows.get(i).cloned());
                self.action = match row {
                    Some(Row::NewBranch) => {
                        Some(ViewerAction::WithHost(ui_components::WithHost::once(|host| {
                            crate::ref_dialog::new_branch(host, None);
                        })))
                    }
                    Some(Row::Branch { current: true, wanted }) => {
                        let name = match &wanted {
                            Wanted::Branch(name) => name.clone(),
                            _ => String::new(),
                        };
                        Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                            host.notify(workbench_model::Notification::info(tr!(
                                "git.switch_already",
                                name = name.as_str()
                            )));
                        })))
                    }
                    Some(Row::Branch { wanted, .. }) => {
                        Some(crate::ref_menu::switch_action(wanted))
                    }
                    None => None,
                };
            }
            Some(PickerEvent::Closed) => {
                self.done = true;
                self.action = Some(ViewerAction::Close);
            }
            _ => {}
        }
    }

    /// Its parts used as controls (the list), with their state of use. The list has the
    /// focus from the moment it opens (its keys, its search, the wheel), and keeps Tab.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut parts: [&mut dyn Control; 1] = [&mut self.picker];
        f(&mut self.controls, &mut parts)
    }

    /// Where it opens from: under the branch, else under the bar's middle.
    fn anchor(&self, bounds: Rect, scale: f32) -> Rect {
        self.list.as_ref().and_then(|l| l.anchor).unwrap_or_else(|| {
            let width = 360.0 * scale;
            Rect::new(
                bounds.min_x() + (bounds.size.width - width) / 2.0,
                bounds.min_y() + 40.0 * scale,
                width,
                0.0,
            )
        })
    }
}

impl Viewer for BranchPicker {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(list) = data.downcast_ref::<BranchList>() else { return };
        let chosen = list.rows.iter().position(|r| matches!(r, Row::Branch { current: true, .. }));
        self.picker.open(list.items.clone(), chosen);
        self.picker.always_search();
        self.list = Some(Arc::new(list.clone()));
        self.with_controls(|c, parts| c.focus(parts, Some(0)));
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.picker.paste(text)
    }

    fn selected_text(&self) -> Option<String> {
        self.picker.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.with_controls(|c, parts| c.key(parts, key, reach));
        let event = self.picker.take_event();
        self.picked(event);
        true
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press(parts, p, clicks));
        let event = self.picker.take_event();
        self.picked(event);
        self.action.take()
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

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some()
    }

    fn hover(&mut self, p: Point) -> bool {
        let (inside, changed) = self.with_controls(|c, parts| {
            let inside = parts.iter().any(|part| part.area().is_some_and(|r| r.contains(p)));
            (inside, c.hover(parts, Some(p)))
        });
        self.inside = inside;
        changed
    }

    fn takes_scroll(&self) -> bool {
        self.inside
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.controls.cursor_at(p).or(Some(ViewerCursor::Arrow))
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds, scale, text, scene, palette, icons, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        let anchor = self.anchor(bounds, scale);
        self.picker.paint(scene, text, icons, &ds, (anchor, bounds), &t("git.branch_search"));
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/branch_picker.rs"]
mod tests;
