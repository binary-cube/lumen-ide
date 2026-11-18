//! A profile as a flame graph drawn top down (plan 39h5, on [`ui_components::icicle`]): a
//! step a bar, as wide as its time in all (its cost on a plan only estimated) within its
//! parent, its own part what its children leave uncovered; its color the heat of its own
//! time, a problem outlined in its severity's color with its sign. Children worth more
//! than their parent (parallel workers, rounding) are scaled into it, "approximate" in the
//! tooltip; bars under 2 px are joined into one "…".
//!
//! A click chooses the step in every view and zooms into it (its subtree the whole width),
//! the band on top saying the way down (each step clickable, ← back); hovering says the
//! exact figures. The bars are made once per profile and findings shown.

use database::profile::derive::ProfileReport;
use database::profile::node::NodeId;
use database::profile::rules::Finding;
use kernel::{Point, Rect};
use ui_components::{
    Icicle, IcicleEvent, IcicleLabels, IcicleNode, IciclePaint, Ink, Tip, ViewerKey,
};

use crate::profile::figures::{self, level};
use crate::profile::{graph, tree};

/// The flame graph's state.
#[derive(Debug, Default)]
pub struct FlameView {
    pub icicle: Icicle,
    made: Option<(database::profile::ProfileId, u64)>,
    area: Rect,
}

/// What a step's bar says: its words (the object after them), its time in all; its tone,
/// its mark, its tooltip.
pub fn nodes(report: &ProfileReport, findings: &[&Finding]) -> Vec<IcicleNode> {
    let timed = report.totals.measure == Some(database::plan::Measure::Time);
    let worst = tree::worst_of(report, findings);
    let p = &report.profile;
    (0..p.nodes.len())
        .map(|i| {
            let id = NodeId(i as u32);
            let node = p.node(id);
            let d = report.derived(id);
            let (value, own, share) = if timed {
                (d.inclusive_ms.unwrap_or(0.0), d.self_ms.unwrap_or(0.0), d.self_pct)
            } else {
                (node.estimates.total_cost.unwrap_or(0.0), d.self_cost.unwrap_or(0.0), d.cost_pct)
            };
            let finding = worst.get(i).copied().flatten().and_then(|f| findings.get(f).copied());
            let label = database::profile::export::head(node);
            let detail = if timed { figures::ms(value) } else { figures::number(value.round()) };
            let detail = match share {
                Some(s) if s >= 0.005 => format!("{detail} · {}", figures::share(s)),
                _ => detail,
            };
            IcicleNode {
                parent: node.parent.map(NodeId::index),
                value,
                self_value: own,
                label,
                detail,
                tone: Ink::heat(share.unwrap_or(0.0)),
                mark: finding.map(|f| Ink::Severity(level(f.severity))),
                sign: finding.map(|f| level(f.severity).icon()),
                tip: graph::card_tip(report, id, finding),
            }
        })
        .collect()
}

/// How the axis writes a value: a time, or a cost.
fn time(v: f64) -> String {
    figures::ms(v)
}

fn cost(v: f64) -> String {
    figures::number(v.round())
}

impl FlameView {
    /// Its bars made again when the profile or the findings shown changed.
    pub fn sync(&mut self, report: &ProfileReport, findings: &[&Finding], generation: u64) {
        let key = (report.profile.id, generation);
        if self.made == Some(key) {
            return;
        }
        let first = self.made.is_none_or(|m| m.0 != report.profile.id);
        self.made = Some(key);
        if first {
            self.icicle = Icicle::new();
        }
        let timed = report.totals.measure == Some(database::plan::Measure::Time);
        self.icicle.set_format(if timed { time } else { cost });
        self.icicle.set_labels(IcicleLabels {
            all: i18n::t("profile.view.flame_all"),
            back: i18n::t("profile.view.flame_back"),
            legend: i18n::t(if timed {
                "profile.view.flame_legend"
            } else {
                "profile.view.flame_legend_cost"
            }),
            merged: i18n::t("profile.view.flame_merged"),
            empty: i18n::t("profile.view.flame_empty"),
            approx: i18n::t("profile.view.approx"),
        });
        self.icicle.set_nodes(nodes(report, findings));
    }

    /// Drawn in `area`, the step chosen in the other views followed.
    pub fn paint(
        &mut self,
        area: Rect,
        scale: f32,
        painter: ui_components::Painter<'_>,
        selected: Option<NodeId>,
    ) {
        let ui_components::Painter { text, scene, palette, icons } = painter;
        self.area = area;
        let selected = selected.map(NodeId::index);
        if self.icicle.selected() != selected {
            self.icicle.select(selected);
        }
        self.icicle.paint(IciclePaint { bounds: area, scale, text, scene, palette, icons });
    }

    /// A press: the step chosen (and zoomed into).
    pub fn press(&mut self, p: Point) -> Option<NodeId> {
        match self.icicle.press(p)? {
            IcicleEvent::Selected(i) => Some(NodeId(i as u32)),
            IcicleEvent::Zoomed => None,
        }
    }

    pub fn key(&mut self, key: ViewerKey) -> bool {
        self.icicle.key(key)
    }

    /// The step chosen by the keys.
    pub fn selected(&self) -> Option<NodeId> {
        self.icicle.selected().map(|i| NodeId(i as u32))
    }

    /// The step whose bar is at `p`.
    pub fn node_at(&self, p: Point) -> Option<NodeId> {
        self.icicle.node_at(p).map(|i| NodeId(i as u32))
    }

    pub fn hover(&mut self, p: Point) -> bool {
        if self.area.contains(p) { self.icicle.hover(p) } else { self.icicle.leave() }
    }

    pub fn scroll(&mut self, dy: f32) -> bool {
        self.icicle.scroll(dy)
    }

    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        self.icicle.tooltip_at(p)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/flame.rs"]
mod tests;
