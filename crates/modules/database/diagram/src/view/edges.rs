//! The edges: the one chosen or under the pointer, its popover, a virtual key accepted or
//! not from it, the edges walked with the keys.

use super::*;

impl RelationsView {
    /// The line selected (its place in the graph shown).
    pub fn selected_edge(&self) -> Option<usize> {
        self.edge.as_ref().and_then(|e| e.find(self.graph.as_deref()?))
    }

    /// The line whose popover is open (its place in the graph shown).
    pub fn popover_edge(&self) -> Option<usize> {
        self.popover.as_ref().and_then(|(e, _)| e.find(self.graph.as_deref()?))
    }

    /// The line at screen point `p` (its place in the graph drawn): the nearest within a
    /// few pixels, scaled with the zoom; none over the bars or out of view.
    pub fn edge_at(&self, p: Point) -> Option<usize> {
        if !self.view.visible().contains(p) || self.view.scale <= 0.0 {
            return None;
        }
        let k = self.view.k();
        let reach = (EDGE_HIT * k).max(EDGE_HIT_MIN * self.view.scale) / k;
        let d = self.view.to_diagram(p);
        self.layout
            .layout
            .edges
            .iter()
            .filter(|e| ui_components::diagram::inflate(e.bounds(), reach).contains(d))
            .map(|e| (e.distance(d), e.kind))
            .filter(|(dist, _)| *dist <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, link)| link)
    }

    /// Line `link` (of the graph drawn) selected, its popover open at `at` (diagram space;
    /// none: its middle).
    pub fn open_edge(&mut self, link: usize, at: Option<Point>) -> bool {
        let Some(key) = EdgeKey::of(&self.drawn, link) else { return false };
        self.edge = Some(key.clone());
        self.popover = Some((key, at));
        true
    }

    /// The popover closed (the line stays selected).
    pub fn close_popover(&mut self) -> bool {
        self.popover.take().is_some()
    }

    /// The key the popover's line stands for (as the decisions keep it).
    fn popover_key(&self) -> Option<VirtualKey> {
        let graph = self.graph.as_deref()?;
        let link = graph.links.get(self.popover_edge()?)?;
        Some(VirtualKey {
            table: graph.tables.get(link.from)?.table().clone(),
            columns: link.columns.clone(),
            target: graph.tables.get(link.to)?.table().clone(),
            references: link.references.clone(),
        })
    }

    /// The popover's line accepted (`Some(true)`), set aside (`Some(false)`), or its
    /// decision undone (`None`).
    pub(super) fn decide_edge(&mut self, accept: Option<bool>) -> bool {
        let Some(key) = self.popover_key() else { return false };
        let done = match accept {
            Some(accept) => match self.found.iter().position(|c| c.key.same(&key)) {
                Some(i) => self.accept(i, accept),
                None => false,
            },
            None => match self.notes.virtual_keys.iter().position(|k| k.same(&key)) {
                Some(i) => self.undo_decision(i, true),
                None => false,
            },
        };
        // Set aside: the line is gone with its popover.
        if accept == Some(false) && done {
            self.popover = None;
            self.edge = None;
        }
        done
    }

    /// The selected table's lines (shown), the next after the line selected (`back`: the
    /// one before) selected.
    pub(super) fn cycle_edges(&mut self, back: bool) -> bool {
        let Some(t) = self.selected else { return false };
        let graph = Arc::clone(&self.drawn);
        let lines: Vec<usize> = self
            .shown
            .links
            .iter()
            .copied()
            .filter(|&l| graph.links.get(l).is_some_and(|l| l.from == t || l.to == t))
            .collect();
        if lines.is_empty() {
            return false;
        }
        let at = self
            .edge
            .as_ref()
            .and_then(|e| e.find(&graph))
            .and_then(|l| lines.iter().position(|&x| x == l));
        let n = lines.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.edge = EdgeKey::of(&graph, lines[next]);
        self.popover = None;
        true
    }
}
