//! The editor groups (split): layout, mirrors, moving tabs between groups.

use super::*;

/// The share of its group's height a view opened below the editor takes (its form and its
/// list need more room than the code above).
const VIEW_BELOW_SHARE: f32 = 0.6;

/// The editor groups (split) and the separators between them, with a tab being dragged.
/// The current group's own state stays in the `Workbench` fields (see [`GroupState`]).
#[derive(Debug)]
pub(super) struct EditorGroups {
    /// The editor groups (at least one); `current` is the current one.
    pub groups: Vec<GroupState>,
    pub current: usize,
    /// How the groups are split (split tree) and the separators between them (the
    /// three-dot handle, the hover animation, the one dragged).
    pub tree: crate::split_tree::SplitTree,
    pub split: super::splits::SplitHandles,
    /// The pressed tab (for dragging) and the drag in progress.
    pub tab_press: Option<(usize, Point)>,
    pub tab_drag: Option<TabDrag>,
}

impl Default for EditorGroups {
    fn default() -> Self {
        Self {
            groups: vec![GroupState::default()],
            current: 0,
            tree: Default::default(),
            split: Default::default(),
            tab_press: None,
            tab_drag: None,
        }
    }
}

impl EditorGroups {
    /// The group at point `p`.
    pub fn group_at(&self, p: Point) -> Option<usize> {
        if self.groups.len() < 2 {
            return None;
        }
        self.groups.iter().position(|g| g.bounds.contains(p))
    }
}

impl Workbench {
    /// The tabs, the breadcrumb and the editor of the current group in `rect`.
    pub(super) fn layout_group(&mut self, rect: Rect) {
        let s = self.scale;
        let tabs_h = if self.tabs.is_empty() { 0.0 } else { TabBar::height(s) };
        self.tab_bar.set_bounds(Rect::new(rect.min_x(), rect.min_y(), rect.size.width, tabs_h), s);
        // The breadcrumb below the tabs, then a small gap: the current line doesn't touch the bar.
        let crumbs_h = if self.tabs.is_empty() { 0.0 } else { Breadcrumbs::height(s) };
        self.breadcrumbs.set_bounds(
            Rect::new(rect.min_x(), rect.min_y() + tabs_h, rect.size.width, crumbs_h),
            s,
        );
        let gap = if self.tabs.is_empty() { 0.0 } else { (EDITOR_GAP * s).round() };
        let top = tabs_h + crumbs_h + gap;
        self.editor_area = Rect::new(
            rect.min_x(),
            rect.min_y() + top,
            rect.size.width,
            (rect.size.height - top).max(0.0),
        );
        for tab in &mut self.tabs {
            tab.sync_bounds(self.editor_area, s);
        }
    }

    /// Swaps the current group's state (from the fields) with the one in slot `g`. Called
    /// twice, it returns to the initial state.
    pub(super) fn swap_group(&mut self, g: usize) {
        let slot = &mut self.editors.groups[g];
        std::mem::swap(&mut self.tabs, &mut slot.tabs);
        std::mem::swap(&mut self.active, &mut slot.active);
        std::mem::swap(&mut self.tab_bar, &mut slot.tab_bar);
        std::mem::swap(&mut self.breadcrumbs, &mut slot.breadcrumbs);
        std::mem::swap(&mut self.crumbs, &mut slot.crumbs);
        std::mem::swap(&mut self.editor_area, &mut slot.editor_area);
        std::mem::swap(&mut self.swatches, &mut slot.swatches);
    }

    /// Makes group `g` current (a click in it). `true` if it changed.
    pub fn focus_group(&mut self, g: usize) -> bool {
        if g == self.editors.current || g >= self.editors.groups.len() {
            return false;
        }
        self.link = None;
        self.error_tip = None;
        self.completion.close();
        self.swap_group(self.editors.current);
        self.swap_group(g);
        self.editors.current = g;
        true
    }

    /// Makes the group at `p` current (e.g. before a tab's context menu).
    pub fn focus_group_at(&mut self, p: Point) -> bool {
        self.editors.group_at(p).is_some_and(|g| self.focus_group(g))
    }

    #[cfg(any(test, feature = "snapshot"))]
    pub fn group_count(&self) -> usize {
        self.editors.groups.len()
    }

    /// Runs `f` with the group at `p` temporarily brought in as the current group (hover, scroll).
    pub(super) fn with_group_at<R>(
        &mut self,
        p: Point,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let g = self.editors.group_at(p).filter(|&g| g != self.editors.current)?;
        self.swap_group(g);
        let result = f(self);
        self.swap_group(g);
        Some(result)
    }

    /// Brings the text of the tabs in the temporarily brought-in group up to date with the
    /// same file in the current group (where typing happens), now in slot `slot`: only the
    /// difference, as one undo step.
    pub(super) fn sync_mirrors(&mut self, slot: usize) {
        let sources = &self.editors.groups[slot].tabs;
        for tab in &mut self.tabs {
            let Some(path) = tab.doc.path() else { continue };
            let Some(source) = sources.iter().find(|t| t.doc.path() == Some(path)) else {
                continue;
            };
            let key = (source.doc.id(), source.doc.version());
            if tab.synced_from == Some(key) {
                continue;
            }
            tab.synced_from = Some(key);
            sync_text(&source.doc, &mut tab.doc);
        }
    }

    /// Splits the editor: tab `i` opens (or, with `move_tab`, moves) in a new group, to
    /// the right or below. `true` if it was done.
    pub fn split_tab(&mut self, i: usize, split: Split, move_tab: bool) -> bool {
        if i >= self.tabs.len() {
            return false;
        }
        // A single tab can't be moved (the group would be left empty): it opens there as well.
        let path = self.tabs[i].doc.path().map(Path::to_path_buf);
        let move_tab = (move_tab && self.tabs.len() > 1) || path.is_none();
        if path.is_none() && self.tabs.len() < 2 {
            return false;
        }
        // A module's view opened again there (a second instance): what it's given to show
        // the same (the Task Manager: its window's resources).
        let mirror = (!move_tab)
            .then(|| self.tabs[i].viewer.as_ref().and_then(|v| v.viewer.mirror_data()))
            .flatten();
        let moved = move_tab.then(|| {
            let tab = self.tabs.remove(i);
            if self.active >= i && self.active > 0 {
                self.active -= 1;
            }
            tab
        });
        let source = self.editors.current;
        let at = source + 1;
        // Only the current group is split (in the tree), the rest stay as they were.
        self.editors.tree.split(source, split);
        self.editors.groups.insert(at, GroupState::default());
        // The current group's slot may have moved (insert before): the indices stay correct,
        // because the new group comes after it.
        self.swap_group(source);
        self.swap_group(at);
        self.editors.current = at;
        match moved {
            Some(tab) => {
                self.tabs.push(tab);
                self.active = 0;
            }
            None if path.as_deref().is_some_and(is_virtual) => {
                let Some(path) = path else { return false };
                if self.open_virtual(path)
                    && let Some(data) = mirror
                {
                    self.set_viewer_data(&*data);
                }
            }
            None => {
                let Some(path) = path else { return false };
                if let Err(e) = self.open_file(&path) {
                    tracing::warn!("can't open {}: {e}", path.display());
                }
                // The text (possibly unsaved) comes from the original tab.
                if let Some(original) =
                    self.editors.groups[source].tabs.iter().find(|t| t.doc.path() == Some(&path))
                    && let Some(tab) = self.tabs.get_mut(self.active)
                {
                    sync_text(&original.doc, &mut tab.doc);
                    tab.synced_from = Some((original.doc.id(), original.doc.version()));
                    let head = original.doc.selections().primary().head.min(tab.doc.buffer().len());
                    commands::click(&mut tab.doc, head, false, false);
                }
            }
        }
        self.focus = Focus::Editor;
        self.layout();
        true
    }

    /// Shows `input` for the active view (the Git log's files, the local history's
    /// versions) above it: the first time, the view moves to a new group at the bottom (its
    /// columns get the whole width) and the comparison opens in the group above; later ones
    /// replace it there. The view keeps the focus; the groups' separator resizes them.
    pub(super) fn compare_beside(&mut self, input: diff::CompareInput, title: &str) {
        let dir = input.right.0.parent().map(Path::to_path_buf).unwrap_or_default();
        let path = dir.join(format!("{title}.{}", diff::COMPARE_VIEW));
        let view = self.editors.current;
        // Where the comparison shown before is, else the group above (or left of) the view.
        let shown_in = self.beside_compare.as_ref().and_then(|old| {
            (0..self.editors.groups.len()).filter(|&g| g != view).find(|&g| {
                self.editors.groups[g].tabs.iter().any(|t| t.doc.path() == Some(old.as_path()))
            })
        });
        let (target, view) = match shown_in.or_else(|| view.checked_sub(1)) {
            Some(target) => (target, view),
            None => {
                // The view moves down into a new group; the comparison goes where it was.
                if self.active >= self.tabs.len() {
                    return;
                }
                let tab = self.tabs.remove(self.active);
                self.active = self.active.saturating_sub(1).min(self.tabs.len().saturating_sub(1));
                let below = view + 1;
                self.editors.tree.split(view, Split::Down);
                self.editors.groups.insert(below, GroupState::default());
                self.swap_group(view);
                self.swap_group(below);
                self.editors.current = below;
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
                self.layout();
                (view, below)
            }
        };
        self.focus_group(target);
        let opened = workbench_model::ViewHost::compare(self, input, title);
        // The one shown before goes (after the new one is in: the group isn't left empty).
        if let Some(old) = self.beside_compare.take().filter(|old| *old != path)
            && let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(old.as_path()))
        {
            self.close_tab(i);
        }
        if opened {
            self.beside_compare = Some(path);
        }
        self.focus_group(view);
        self.focus = Focus::Editor;
        self.layout();
    }

    /// The group of the views opened below the editor (the current one first).
    fn below_group(&self) -> Option<usize> {
        let below = |tabs: &[Tab]| tabs.iter().any(|t| t.viewer.as_ref().is_some_and(|v| v.below));
        if below(&self.tabs) {
            return Some(self.editors.current);
        }
        (0..self.editors.groups.len())
            .filter(|&g| g != self.editors.current)
            .find(|&g| below(&self.editors.groups[g].tabs))
    }

    /// Opens a module's view in a tab of the group below the editor: the one already holding
    /// such views, else a new one split under the current group (none with an empty editor).
    pub(super) fn open_view_below(
        &mut self,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let dir = self.project.as_ref().map(|p| p.root_path().to_path_buf()).unwrap_or_default();
        let path = dir.join(format!("{title}.{extension}"));
        match self.below_group() {
            Some(g) => {
                self.focus_group(g);
            }
            None if !self.tabs.is_empty() => {
                let source = self.editors.current;
                let below = source + 1;
                self.editors.tree.split_sharing(source, Split::Down, VIEW_BELOW_SHARE);
                self.editors.groups.insert(below, GroupState::default());
                self.swap_group(source);
                self.swap_group(below);
                self.editors.current = below;
            }
            None => {}
        }
        if !self.open_virtual(path) {
            self.drop_empty_group();
            return false;
        }
        if let Some(viewer) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut()) {
            viewer.below = true;
        }
        self.set_viewer_data(data);
        self.focus = Focus::Editor;
        self.layout();
        true
    }

    /// The code a view below the editor opens goes to the group above it (the one it was
    /// split from), the view kept in sight.
    pub(super) fn focus_code_group(&mut self) {
        let below =
            self.tabs.get(self.active).and_then(|t| t.viewer.as_ref()).is_some_and(|v| v.below);
        if below
            && let Some(above) =
                self.editors.current.checked_sub(1).or((self.editors.groups.len() > 1).then_some(1))
        {
            self.focus_group(above);
        }
    }

    /// Closes the current group if it was left empty (and isn't the only one).
    pub(super) fn drop_empty_group(&mut self) {
        if !self.tabs.is_empty() || self.editors.groups.len() < 2 {
            return;
        }
        // The current slot is empty; the current (empty) state is dropped with it.
        self.editors.groups.remove(self.editors.current);
        self.editors.tree.remove(self.editors.current);
        let next = self.editors.current.min(self.editors.groups.len() - 1);
        self.swap_group(next);
        self.editors.current = next;
        self.layout();
    }

    /// Merges all groups into one (e.g. before closing the window, so the save dialogs
    /// see all the tabs). Copies of the same file are kept once.
    pub fn merge_groups(&mut self) {
        if self.editors.groups.len() < 2 {
            return;
        }
        for g in 0..self.editors.groups.len() {
            if g == self.editors.current {
                continue;
            }
            for tab in std::mem::take(&mut self.editors.groups[g].tabs) {
                let duplicate = tab.doc.path().is_some()
                    && self.tabs.iter().any(|t| t.doc.path() == tab.doc.path());
                if duplicate {
                    if let Some(worker) = &self.worker {
                        worker.forget(tab.doc.id());
                    }
                } else {
                    self.tabs.push(tab);
                }
            }
        }
        let current = std::mem::take(&mut self.editors.groups[self.editors.current]);
        self.editors.groups = vec![GroupState { bounds: current.bounds, ..GroupState::default() }];
        self.editors.tree = Default::default();
        self.editors.current = 0;
        self.layout();
    }

    /// The separators between groups (the `Splitter` component: line and handle).
    pub(super) fn paint_group_dividers(&self, scene: &mut Scene) {
        self.editors.split.paint(scene);
    }

    /// Where a tab dropped at `p` would land: (the group, the position, the insertion line).
    pub(super) fn tab_drop_target(&self, p: Point) -> Option<(usize, usize, Option<Rect>)> {
        let groups = self.editors.groups.len();
        for g in 0..groups {
            let (bar, count) = if g == self.editors.current {
                (&self.tab_bar, self.tabs.len())
            } else {
                (&self.editors.groups[g].tab_bar, self.editors.groups[g].tabs.len())
            };
            if let Some((index, marker)) = bar.drop_target(p) {
                return Some((g, index, Some(marker)));
            }
            // In another group's editor area: the tab is added at the end.
            let bounds = if groups > 1 { self.editors.groups[g].bounds } else { Rect::default() };
            if g != self.editors.current && bounds.contains(p) {
                return Some((g, count, None));
            }
        }
        None
    }

    /// The dragged tab was dropped: another position in the group or another group.
    pub(super) fn drop_tab(&mut self, drag: &TabDrag) {
        let Some((g, index, _)) = drag.target else { return };
        let from = drag.tab;
        if from >= self.tabs.len() {
            return;
        }
        if g == self.editors.current {
            let to = if index > from { index - 1 } else { index }.min(self.tabs.len() - 1);
            if to != from {
                let tab = self.tabs.remove(from);
                self.tabs.insert(to, tab);
                self.active = to;
            }
            self.layout();
            return;
        }
        let tab = self.tabs.remove(from);
        if self.active >= from && self.active > 0 {
            self.active -= 1;
        }
        let source = self.editors.current;
        self.focus_group(g);
        let existing =
            tab.doc.path().and_then(|p| self.tabs.iter().position(|t| t.doc.path() == Some(p)));
        match existing {
            // The same file was already there (a synced copy): that one stays.
            Some(i) => {
                if let Some(worker) = &self.worker {
                    worker.forget(tab.doc.id());
                }
                self.activate(i);
            }
            None => {
                let index = index.min(self.tabs.len());
                self.tabs.insert(index, tab);
                self.active = index;
            }
        }
        self.focus = Focus::Editor;
        if self.editors.groups[source].tabs.is_empty() {
            self.editors.groups.remove(source);
            self.editors.tree.remove(source);
            if self.editors.current > source {
                self.editors.current -= 1;
            }
        }
        self.layout();
    }

    /// The dragged tab: a pill with its name next to the cursor and the spot it would go into.
    pub(super) fn paint_tab_drag(&self, text: &mut TextSystem, scene: &mut Scene) {
        let Some(drag) = &self.editors.tab_drag else { return };
        let p = &self.palette;
        let layer = scene.push_layer(None);
        match drag.target {
            Some((_, _, Some(marker))) => {
                layer.quad(Quad::filled(marker, p.accent).rounded(marker.size.width / 2.0));
            }
            Some((g, _, None)) => {
                super::splits::paint_drop_zone(layer, self.editors.groups[g].bounds, self.scale, p);
            }
            None => {}
        }
        self.paint_drag_pill(text, scene, &drag.title, drag.icon, drag.at);
    }

    /// A dragged thing's pill by the cursor: its icon and name.
    pub(super) fn paint_drag_pill(
        &self,
        text: &mut TextSystem,
        scene: &mut Scene,
        title: &str,
        icon_of: Option<(IconId, Rgba)>,
        at: Point,
    ) {
        let ctx = ui_workbench::ds::Ctx::new(&self.palette, self.scale);
        let p = &self.palette;
        let layer = scene.push_layer(None);
        let font = ctx.ui_font(-0.5);
        let shaped = text.shape_line(title, &font);
        let icon = ctx.px(15.0);
        let h = ctx.px(32.0);
        let w = ctx.px(14.0) + icon + ctx.px(8.0) + shaped.width + ctx.px(14.0);
        let r = Rect::new(at.x + ctx.px(10.0), at.y + ctx.px(8.0), w, h);
        ui_workbench::ds::surface(layer, &ctx, r, ui_workbench::ds::Elevation::Medium, 9.0);
        let mut x = r.min_x() + ctx.px(14.0);
        if let Some((id, color)) = icon_of {
            draw_icon(
                layer,
                &self.icons,
                id,
                Rect::new(x, r.min_y() + (h - icon) / 2.0, icon, icon),
                color,
            );
            x += icon + ctx.px(8.0);
        }
        draw_text(
            layer,
            &shaped,
            x,
            ui_workbench::ds::optical_baseline(r.min_y(), h, &font),
            p.text,
        );
    }

    /// The snapshot's: the first split's separator placed `at` logical pixels from the
    /// window's left edge (or top, for a split down) — a view's exact width.
    #[cfg(feature = "snapshot")]
    pub fn place_first_divider(&mut self, at: f32) {
        let Some(divider) = self.editors.split.dividers.first().cloned() else { return };
        let p = at * self.scale;
        self.editors.tree.drag(&divider, Point::new(p, p));
        self.layout();
        let widths: Vec<f32> =
            self.editors.groups.iter().map(|g| g.bounds.size.width / self.scale).collect();
        tracing::info!(?widths, "the first separator placed");
    }

    /// The snapshot's: the left side panel (the project's tree) closed, the editor as wide as
    /// the window lets it.
    #[cfg(feature = "snapshot")]
    pub fn close_left_panel(&mut self) {
        let left =
            self.tools.sides.iter().find(|s| s.dock == ui_workbench::Dock::Left).map(|s| s.id);
        if let Some(id) = left {
            self.close_side_panel(id);
            self.layout();
        }
    }

    // ─── The project session ───
}
