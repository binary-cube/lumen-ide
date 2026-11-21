//! Dragging in the list: a project onto a group (or out of one), a group into another; where it
//! lands, the list scrolled at its edges.

use super::*;

impl ProjectsPicker {
    /// The scrolling a drag near the list's top (negative) or bottom edge asks for, in
    /// pixels per second: faster closer to the edge (or past it).
    pub(super) fn edge_speed(&self) -> f32 {
        let Some(drag) = &self.drag else { return 0.0 };
        let scale = self.scale.max(1.0);
        let zone = EDGE * scale;
        let above = drag.at.y - self.list.min_y();
        let below = self.list.max_y() - drag.at.y;
        let speed = |distance: f32| (1.0 - distance.max(0.0) / zone) * EDGE_SPEED * scale;
        if above < zone && self.scroll.offset() > 0.0 {
            -speed(above)
        } else if below < zone && self.scroll.offset() < self.scroll.max() {
            speed(below)
        } else {
            0.0
        }
    }

    /// What's under `p`, for its right-click menu.
    pub fn target_at(&self, p: Point) -> Option<PickerTarget> {
        if !self.list.contains(p) {
            return None;
        }
        if self.history.is_some_and(|r| r.contains(p)) {
            return Some(PickerTarget::History);
        }
        let (_, i) = self.rows.iter().find(|(r, _)| r.contains(p))?;
        match self.items.get(*i)? {
            Item::Project { project, current, .. } => {
                Some(PickerTarget::Project { path: project.path.clone(), current: *current })
            }
            Item::Group { id, .. } => Some(PickerTarget::Group(*id)),
            _ => None,
        }
    }

    /// Where project `path`'s row was painted (snapshots point at it).
    pub fn project_row(&self, path: &std::path::Path) -> Option<Rect> {
        let at = self
            .items
            .iter()
            .position(|i| matches!(i, Item::Project { project, .. } if project.path == path))?;
        self.rows.iter().find(|(_, i)| *i == at).map(|(r, _)| *r)
    }

    /// A press on a project or a group waits to become a click or a drag (or one in the
    /// filter selects while dragged).
    pub fn pressed(&self) -> bool {
        self.press.is_some() || self.selecting
    }

    /// The mouse moved while pressed: the row is dragged (past a few pixels), over where it
    /// would go. `true`: redraw.
    pub fn drag(&mut self, p: Point) -> bool {
        if self.selecting {
            return Control::drag(&mut self.query, p);
        }
        let Some((item, start)) = self.press else { return false };
        let moved = (p.x - start.x).abs().max((p.y - start.y).abs());
        if self.drag.is_none() && moved < DRAG_START * self.scale.max(1.0) {
            return false;
        }
        let drop = self.drop_target(item, p);
        self.drag = Some(Drag { item, at: p, drop });
        true
    }

    /// The press ends: a click (the row's action) or a drop (a move).
    pub fn release(&mut self) -> Option<PickerAction> {
        self.selecting = false;
        let (item, _) = self.press.take()?;
        self.edge_step = None;
        match self.drag.take() {
            Some(drag) => drag.drop.map(|d| d.action),
            None => self.items.get(item).and_then(Item::action),
        }
    }

    /// Where item `item` dropped at `p` goes:
    /// - a project: into a group (its row, or the title: the recent group), or between two
    ///   projects of a group (before the upper half's, after the lower half's), never in
    ///   the recent group (always by opening);
    /// - a group: into another (its middle), before or after it (its top or bottom
    ///   quarter; collapsed, else into), or the top (the title); never into itself, its
    ///   subgroups or the recent group.
    pub(super) fn drop_target(&self, item: usize, p: Point) -> Option<Drop> {
        if !self.list.contains(p) {
            return None;
        }
        let dragged = self.items.get(item)?;
        if self.history.is_some_and(|r| r.contains(p)) {
            let action = match dragged {
                Item::Project { project, .. } => PickerAction::MoveProject {
                    path: project.path.clone(),
                    into: RECENT,
                    place: Place::Anywhere,
                },
                Item::Group { id, .. } => {
                    PickerAction::MoveGroup { id: *id, parent: None, before: None }
                }
                _ => return None,
            };
            return Some(Drop { action, mark: Mark::Header });
        }
        let &(r, i) = self.rows.iter().find(|(r, _)| r.contains(p))?;
        let rel = (p.y - r.min_y()) / r.size.height.max(1.0);
        let px = |v: f32| v * self.scale.max(1.0);
        let line = |y: f32| Mark::Line { x: r.min_x() + px(10.0), y };
        match (dragged, self.items.get(i)?) {
            (Item::Project { project, .. }, Item::Group { id, .. }) => Some(Drop {
                action: PickerAction::MoveProject {
                    path: project.path.clone(),
                    into: *id,
                    place: Place::Anywhere,
                },
                mark: Mark::Row(i),
            }),
            (
                Item::Project { project, .. },
                Item::Project { project: over, group: Some(g), .. },
            ) => {
                let path = project.path.clone();
                if *g == RECENT {
                    let row = self.group_row(RECENT)?;
                    let action =
                        PickerAction::MoveProject { path, into: RECENT, place: Place::Anywhere };
                    return Some(Drop { action, mark: Mark::Row(row) });
                }
                if over.path == path {
                    return None;
                }
                let (place, y) = if rel < 0.5 {
                    (Place::Before(over.path.clone()), r.min_y())
                } else {
                    let next = self.next_project(i, &path);
                    (next.map_or(Place::End, Place::Before), r.max_y())
                };
                let action = PickerAction::MoveProject { path, into: *g, place };
                Some(Drop { action, mark: line(y) })
            }
            (Item::Group { id, .. }, Item::Group { id: over, collapsed, .. }) => {
                if self.within(*over, *id) {
                    return None;
                }
                let parent = self.groups.iter().find(|g| g.id == *over).and_then(|g| g.parent);
                let fixed = *over == RECENT;
                let (parent, before, mark) = if rel < 0.25 {
                    (parent, Some(*over), line(r.min_y()))
                } else if fixed || (rel > 0.75 && *collapsed) {
                    let y = self.subtree_bottom(i).unwrap_or(r.max_y());
                    (
                        parent,
                        self.next_sibling(*over, *id),
                        Mark::Line { x: r.min_x() + px(10.0), y },
                    )
                } else {
                    (Some(*over), None, Mark::Row(i))
                };
                Some(Drop { action: PickerAction::MoveGroup { id: *id, parent, before }, mark })
            }
            _ => None,
        }
    }

    /// The row of group `id`'s item.
    pub(super) fn group_row(&self, id: GroupId) -> Option<usize> {
        self.items.iter().position(|i| matches!(i, Item::Group { id: g, .. } if *g == id))
    }

    /// The project shown after row `i` in its group (`skip`, the one dragged, isn't one).
    pub(super) fn next_project(&self, i: usize, skip: &Path) -> Option<PathBuf> {
        let Some(Item::Project { group, .. }) = self.items.get(i) else { return None };
        self.items[i + 1..]
            .iter()
            .map_while(|item| match item {
                Item::Project { project, group: g, .. } if g == group => Some(&project.path),
                _ => None,
            })
            .find(|p| p.as_path() != skip)
            .cloned()
    }

    /// The sibling after group `id` (`skip`, the one dragged, isn't one).
    pub(super) fn next_sibling(&self, id: GroupId, skip: GroupId) -> Option<GroupId> {
        let parent = self.groups.iter().find(|g| g.id == id)?.parent;
        let at = self.groups.iter().position(|g| g.id == id)?;
        self.groups[at + 1..].iter().find(|g| g.parent == parent && g.id != skip).map(|g| g.id)
    }

    /// The bottom of the last row shown inside group row `i` (expanded).
    pub(super) fn subtree_bottom(&self, i: usize) -> Option<f32> {
        let Some(Item::Group { depth, .. }) = self.items.get(i) else { return None };
        let inside = |item: &Item| match item {
            Item::Group { depth: d, .. } | Item::Project { depth: d, group: Some(_), .. } => {
                *d > *depth
            }
            _ => false,
        };
        let last = i + self.items[i + 1..].iter().take_while(|item| inside(item)).count();
        (last > i).then(|| self.rows.iter().find(|(_, j)| *j == last).map(|(r, _)| r.max_y()))?
    }

    /// `group` is `ancestor` or inside it.
    pub(super) fn within(&self, group: GroupId, ancestor: GroupId) -> bool {
        let mut at = Some(group);
        for _ in 0..=self.groups.len() {
            match at {
                Some(g) if g == ancestor => return true,
                Some(g) => at = self.groups.iter().find(|x| x.id == g).and_then(|x| x.parent),
                None => return false,
            }
        }
        false
    }

    /// A drag held near the list's edge scrolls it, by the time since the last step.
    pub(super) fn scroll_at_edge(&mut self) {
        let speed = self.edge_speed();
        if speed == 0.0 {
            self.edge_step = None;
            return;
        }
        let now = Instant::now();
        // At most a frame's worth after a pause (the first step waits for the next frame).
        let dt = self.edge_step.map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.05));
        self.edge_step = Some(now);
        self.scroll.scroll_by(speed * dt);
    }
}
