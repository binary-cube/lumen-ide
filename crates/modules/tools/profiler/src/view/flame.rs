//! The flame graph of a recording: the timed work as it nested (the frame, the poll, then
//! each view's paint or poll, then what it timed itself), a node per way down, as wide as
//! its time with what it held, its own part what's left uncovered; the heat its share of
//! the whole. Laid out by the shared icicle (`ui_components::Icicle`: a click zooms in, its
//! band leads back, its keys walk it).

use i18n::{t, tr};
use profiling::Summary;
use ui_components::tree_table::Ink;
use ui_components::{Icicle, IcicleLabels, IcicleNode};

/// An empty icicle with our words and our time's format.
pub fn icicle() -> Icicle {
    let mut icicle = Icicle::new();
    icicle.set_format(|us| format!("{:.1} ms", us / 1000.0));
    icicle.set_labels(IcicleLabels {
        all: t("profiler.flame_all"),
        back: t("profiler.flame_back"),
        legend: t("profiler.flame_legend"),
        merged: t("profiler.flame_merged"),
        empty: t("profiler.flame_none"),
        approx: t("profiler.flame_approx"),
    });
    icicle
}

/// The nodes of `s`'s ways down: each one's parent the way one step shorter, its value its
/// time (microseconds), its own what its children don't cover.
pub fn nodes(s: &Summary) -> Vec<IcicleNode> {
    let mut ways: Vec<&(Vec<String>, u64, u64)> = s.spans.iter().collect();
    ways.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(b.2.cmp(&a.2)));
    let total: u64 = ways.iter().filter(|w| w.0.len() == 1).map(|w| w.2).sum::<u64>().max(1);
    let mut nodes: Vec<IcicleNode> = Vec::new();
    let mut places: Vec<&[String]> = Vec::new();
    for (path, calls, us) in ways {
        let Some(last) = path.last() else { continue };
        let parent = places.iter().position(|p| *p == &path[..path.len() - 1]);
        if path.len() > 1 && parent.is_none() {
            continue;
        }
        let ms = format!("{:.1} ms", *us as f64 / 1000.0);
        nodes.push(IcicleNode {
            parent,
            value: *us as f64,
            self_value: *us as f64,
            label: last.clone(),
            detail: ms.clone(),
            tone: Ink::heat(*us as f64 / total as f64),
            mark: None,
            sign: None,
            tip: tr!(
                "profiler.flame_tip",
                step = last.as_str(),
                ms = ms.as_str(),
                calls = calls.to_string().as_str()
            ),
        });
        places.push(path.as_slice());
    }
    // Each one's own part: what its children don't cover.
    for i in 0..nodes.len() {
        let children: f64 = nodes.iter().filter(|n| n.parent == Some(i)).map(|n| n.value).sum();
        nodes[i].self_value = (nodes[i].value - children).max(0.0);
    }
    nodes
}
