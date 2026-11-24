//! The mouse over the diagram: the card or control under it, a press (choose, fold, a
//! control), the pan, the wheel, the tooltip, the arrows walking the steps.

use super::*;

impl GraphView {
    /// The card at screen point `p`.
    fn card_at(&self, p: Point) -> Option<usize> {
        if !self.area.contains(p) {
            return None;
        }
        let d = self.view.to_diagram(p);
        self.laid()?.cards.iter().position(|c| c.rect.contains(d))
    }

    pub(super) fn hit(&self, p: Point) -> Option<GraphHit> {
        if let Some((_, h, _)) = self.controls.iter().find(|(r, ..)| r.contains(p)) {
            return Some(*h);
        }
        self.card_at(p).map(GraphHit::Card)
    }

    /// A press: a control acts at once (`true`); a double click on a card folds it; else a
    /// pan may begin (a click told at the release).
    pub fn press(
        &mut self,
        p: Point,
        clicks: u32,
        selected: Option<NodeId>,
    ) -> (bool, Option<GraphEvent>) {
        match self.hit(p) {
            Some(GraphHit::ZoomIn) => return (self.zoom_by(1.25, None), None),
            Some(GraphHit::ZoomOut) => return (self.zoom_by(0.8, None), None),
            Some(GraphHit::Zoom) => {
                return (self.view.reset_zoom(ZOOM_MIN, ZOOM_MAX), None);
            }
            Some(GraphHit::Fit) => {
                self.fit(selected);
                return (true, None);
            }
            Some(GraphHit::Center) => return (selected.is_some_and(|s| self.center_on(s)), None),
            Some(GraphHit::Flow) => {
                self.turn();
                return (true, None);
            }
            Some(GraphHit::Card(c)) if clicks >= 2 => {
                if let Some(Item::Step(id)) =
                    self.laid().and_then(|l| l.cards.get(c)).map(|c| c.item)
                {
                    return (true, Some(GraphEvent::Toggle(id)));
                }
            }
            _ => {}
        }
        if self.area.contains(p) {
            self.view.press(p);
        }
        (false, None)
    }

    pub fn drag(&mut self, p: Point) -> bool {
        self.view.drag(p)
    }

    /// The mouse released: a click on a card chooses its step (a "+N" one opens it).
    pub fn release(&mut self) -> Option<GraphEvent> {
        let Release::Click(at) = self.view.release() else { return None };
        let c = self.card_at(at)?;
        match self.laid()?.cards.get(c)?.item {
            Item::Step(id) => Some(GraphEvent::Selected(id)),
            Item::Folded { under, .. } => Some(GraphEvent::Toggle(under)),
        }
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view.scroll(dx, dy);
        true
    }

    /// The step whose card is at `p`.
    pub fn node_at(&self, p: Point) -> Option<NodeId> {
        let c = self.card_at(p)?;
        match self.laid()?.cards.get(c)?.item {
            Item::Step(id) => Some(id),
            Item::Folded { .. } => None,
        }
    }

    pub fn is_control(&self, p: Point) -> bool {
        self.controls.iter().any(|(r, ..)| r.contains(p))
    }

    pub fn is_over_card(&self, p: Point) -> bool {
        self.card_at(p).is_some()
    }

    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        if let Some((r, _, tip)) = self.controls.iter().find(|(r, ..)| r.contains(p)) {
            return Some(Tip::new(*r, tip.clone()));
        }
        if !self.area.contains(p) {
            return None;
        }
        let (r, tip) = self.tips.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(Tip::new(*r, tip.clone()))
    }

    /// The step the arrows lead to from `from`: the parent toward the roots, the first
    /// child away, the siblings across (by the flow's axes).
    pub fn step_from(
        report: &ProfileReport,
        from: NodeId,
        key: ui_components::ViewerKey,
        flow: Flow,
    ) -> Option<NodeId> {
        use ui_components::ViewerKey as K;
        let p = &report.profile;
        let node = p.node(from);
        let (to_parent, to_child, back, forth) = match flow {
            Flow::Down => (K::Up, K::Down, K::Left, K::Right),
            Flow::Right => (K::Left, K::Right, K::Up, K::Down),
        };
        let siblings = |id: NodeId| match p.node(id).parent {
            Some(parent) => p.node(parent).children.clone(),
            None => p.roots.clone(),
        };
        if key == to_parent {
            node.parent
        } else if key == to_child {
            node.children.first().copied()
        } else if key == back || key == forth {
            let s = siblings(from);
            let at = s.iter().position(|&x| x == from)?;
            let to = if key == back { at.checked_sub(1)? } else { at + 1 };
            s.get(to).copied()
        } else {
            None
        }
    }
}
