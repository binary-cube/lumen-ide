//! A tool window tab dragged: where it can land and what dropping it does.

use super::*;

impl Workbench {
    /// A press on tab `i` of group `g` of the shown window at `p`: it may be dragged among
    /// the others (their places as last painted). `false` if they weren't painted yet.
    pub(super) fn press_tool_tab(&mut self, g: usize, i: usize, p: Point) -> bool {
        let Some(group) = self.tools.window().and_then(|w| w.groups.get(g)) else { return false };
        let rects: Option<Vec<Rect>> = (0..group.views.len())
            .map(|j| {
                self.tools.hits.iter().find_map(|(r, h)| (*h == ToolHit::View(g, j)).then_some(*r))
            })
            .collect();
        let (Some(rects), Some(view)) = (rects, group.views.get(i)) else { return false };
        let (Some(first), Some(pressed)) = (rects.first(), rects.get(i)) else { return false };
        let gap = rects.get(1).map_or(0.0, |second| second.min_x() - first.max_x());
        self.tools.tab_drag = Some(ToolTabDrag {
            view: view.id,
            group: g,
            origin: i,
            press: p,
            grab: p.x - pressed.min_x(),
            left: pressed.min_x(),
            start: first.min_x(),
            gap,
            widths: rects.iter().map(|r| r.size.width).collect(),
            state: TabDragState::Pressed,
            at: p,
            in_strip: true,
            target: None,
        });
        true
    }

    /// The pressed tab dragged to `p`: past the threshold, over its own strip it follows
    /// the pointer (horizontally), moving among the others as it crosses their middles (the
    /// pinned ones stay first); elsewhere it's back in its place and the drop target is
    /// found (another group's tabs, its view, a side of a view). `true` if a redraw is needed.
    pub(super) fn drag_tool_tab(&mut self, p: Point) -> bool {
        let threshold = super::splits::DRAG_THRESHOLD * self.scale;
        let Some(drag) = self.tools.tab_drag.as_mut() else { return false };
        match drag.state {
            TabDragState::Cancelled => return false,
            TabDragState::Pressed => {
                let moved = (p.x - drag.press.x).abs().max((p.y - drag.press.y).abs());
                if moved < threshold {
                    return false;
                }
                drag.state = TabDragState::Moving;
                self.tools.hover = None;
            }
            TabDragState::Moving => {}
        }
        let target = self.tool_drop_target(p);
        let id = self.tools.bottom.clone();
        let Some(drag) = self.tools.tab_drag.as_mut() else { return false };
        drag.at = p;
        let group = self
            .tools
            .windows
            .iter_mut()
            .find(|w| Some(&w.id) == id.as_ref())
            .and_then(|w| w.groups.get_mut(drag.group));
        let Some(group) = group else {
            self.tools.tab_drag = None;
            return true;
        };
        // The group's views changed meanwhile (a shell ended): the drag stops there.
        let at = group.views.iter().position(|v| v.id == drag.view);
        let Some(at) = at.filter(|_| group.views.len() == drag.widths.len()) else {
            self.tools.tab_drag = None;
            return true;
        };
        let own = matches!(target, Some(ToolDrop::Strip { group, .. }) if group == drag.group);
        drag.in_strip = own;
        drag.target = if own { None } else { target };
        let to = if own {
            drag.left = p.x - drag.grab;
            let pinned = group.views.iter().filter(|v| v.pinned).count();
            let range = if group.views[at].pinned {
                0..=pinned.saturating_sub(1)
            } else {
                pinned..=group.views.len() - 1
            };
            let left = drag.floating_left(at);
            tab_drop_index(&drag.widths, at, left, drag.start, drag.gap, range)
        } else {
            // Out of its strip: back in its place meanwhile.
            drag.origin.min(group.views.len() - 1)
        };
        if to != at {
            let view = group.views.remove(at);
            group.views.insert(to, view);
            let width = drag.widths.remove(at);
            drag.widths.insert(to, width);
        }
        group.active = to;
        true
    }

    /// Where the tab dragged to `p` would go (see [`ToolDrop`]); its own strip is a
    /// `Strip` of its own group (it moves there itself). `None`: nowhere (outside the
    /// window, its own view's middle, or a side of its own group when it's alone there).
    pub(super) fn tool_drop_target(&self, p: Point) -> Option<ToolDrop> {
        let drag = self.tools.tab_drag.as_ref()?;
        let window = self.tools.window()?;
        let dragged = window.groups.get(drag.group)?.views.iter().find(|v| v.id == drag.view)?;
        let header = self.tool_header_height();
        let px = |v: f32| (v * self.scale).round();
        for (g, group) in window.groups.iter().enumerate() {
            let b = group.bounds;
            let strip = Rect::new(b.min_x(), b.min_y(), b.size.width, header);
            if strip.contains(p) {
                if g == drag.group {
                    return Some(ToolDrop::Strip { group: g, index: 0, marker: Rect::default() });
                }
                // Before the first tab whose middle is right of the pointer; the pinned ones
                // stay first.
                let tabs: Vec<Rect> = (0..group.views.len())
                    .filter_map(|j| {
                        self.tools
                            .hits
                            .iter()
                            .find_map(|(r, h)| (*h == ToolHit::View(g, j)).then_some(*r))
                    })
                    .collect();
                let mut index =
                    tabs.iter().filter(|r| r.min_x() + r.size.width / 2.0 < p.x).count();
                let pinned = group.views.iter().filter(|v| v.pinned).count();
                index = if dragged.pinned { index.min(pinned) } else { index.max(pinned) };
                let gap = px(4.0);
                let x = match (tabs.get(index), tabs.last()) {
                    (Some(tab), _) => tab.min_x() - gap / 2.0,
                    (None, Some(last)) => last.max_x() + gap / 2.0,
                    (None, None) => b.min_x() + px(8.0),
                };
                let line = px(2.0).max(1.0);
                let mid = b.min_y() + header / 2.0;
                let marker = Rect::new(x - line / 2.0, mid - px(12.0), line, px(24.0));
                return Some(ToolDrop::Strip { group: g, index, marker });
            }
            let view = self.tool_group_view_bounds(b);
            if view.contains(p) {
                let alone = g == drag.group && group.views.len() < 2;
                return match drop_side(view, p) {
                    Some((side, zone)) if !alone => Some(ToolDrop::Edge { group: g, side, zone }),
                    None if g != drag.group => Some(ToolDrop::Into { group: g, zone: view }),
                    _ => None,
                };
            }
        }
        None
    }

    /// The dragged view `id` dropped on `target`: into another group (its tabs at a place,
    /// or the end), or into a new group split off on a side of one. The group it leaves
    /// goes if it's left empty; where it lands becomes current.
    pub(super) fn drop_tool_tab(&mut self, id: ViewId, target: ToolDrop) {
        let Some(window) = self.tools.window_mut() else { return };
        let Some((source, i)) = window.find(|v| v.id == id) else { return };
        let Some(view) = window.groups[source].remove(i) else { return };
        // Into a group: at a place among its tabs, or at their end.
        let into = match target {
            ToolDrop::Strip { group, index, .. } => Some((group, index)),
            ToolDrop::Into { group, .. } if group != source => Some((group, usize::MAX)),
            _ => None,
        };
        match (into, target) {
            (Some((group, index)), _) => {
                let into = &mut window.groups[group];
                let index = index.min(into.views.len());
                into.views.insert(index, view);
                into.active = index;
                window.current = group;
            }
            (None, ToolDrop::Edge { group, side, .. }) => {
                let (dir, before) = side.split();
                let new = if before {
                    window.tree.split_before(group, dir);
                    group
                } else {
                    window.tree.split(group, dir);
                    group + 1
                };
                window.groups.insert(new, ToolGroup::default());
                window.groups[new].views.push(view);
                window.current = new;
            }
            // Its own group's middle: back where it was.
            (None, _) => {
                window.groups[source].views.insert(i, view);
                window.groups[source].active = i;
            }
        }
        window.prune();
        self.focus = Focus::Panel;
        self.layout();
    }

    /// Esc while a tab of the bottom window is dragged: it goes back where it was (the rest
    /// of the drag does nothing). `false` if none is dragged.
    pub fn cancel_tool_tab_drag(&mut self) -> bool {
        let id = self.tools.bottom.clone();
        let Some(drag) = self.tools.tab_drag.as_mut().filter(|d| d.state == TabDragState::Moving)
        else {
            return false;
        };
        drag.state = TabDragState::Cancelled;
        drag.target = None;
        drag.in_strip = true;
        let group = self
            .tools
            .windows
            .iter_mut()
            .find(|w| Some(&w.id) == id.as_ref())
            .and_then(|w| w.groups.get_mut(drag.group));
        if let Some(group) = group
            && let Some(at) = group.views.iter().position(|v| v.id == drag.view)
        {
            let view = group.views.remove(at);
            let origin = drag.origin.min(group.views.len());
            group.views.insert(origin, view);
            group.active = origin;
        }
        true
    }
}
