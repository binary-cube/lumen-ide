//! The profile's flame graph: a bar per step as wide as its time in all (its cost on a plan
//! estimated), colored by its own share, a problem outlined with its sign; made once per
//! profile and findings shown.

use super::*;
use database::profile::rules::Finding;

#[test]
fn bars_follow_the_steps_times_and_problems() {
    let report = crate::profile::sample::report(crate::profile::sample::SQL);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let bars = nodes(&report, &findings);
    assert_eq!(bars.len(), report.profile.nodes.len());
    let worst = tree::worst_of(&report, &findings);
    for (i, bar) in bars.iter().enumerate() {
        let id = NodeId(i as u32);
        let d = report.derived(id);
        assert_eq!(bar.value, d.inclusive_ms.unwrap());
        assert_eq!(bar.self_value, d.self_ms.unwrap());
        assert_eq!(bar.parent, report.profile.node(id).parent.map(NodeId::index));
        assert_eq!(bar.tone, Ink::heat(d.self_pct.unwrap()));
        assert_eq!(bar.mark.is_some(), worst[i].is_some());
        assert_eq!(bar.sign.is_some(), worst[i].is_some());
    }
    let hot = report.hot_path.last().unwrap().index();
    assert!(bars[hot].label.contains("public.orders"));
    assert!(bars[hot].detail.contains("60"), "{}", bars[hot].detail);
}

#[test]
fn an_estimated_plan_is_drawn_by_its_cost() {
    use database::profile::testing::{ProfileBuilder, planned};
    let profile = ProfileBuilder::new("postgres", database::profile::ProfileDepth::Estimated)
        .child(database::profile::node::OpKind::Sort, "Sort", |n| planned(n, 10.0, 120.0, 5.0))
        .node(database::profile::node::OpKind::SeqScan, "Seq Scan", |n| planned(n, 0.0, 100.0, 5.0))
        .build();
    let report = database::profile::derive(profile);
    let bars = nodes(&report, &[]);
    assert_eq!((bars[0].value, bars[1].value), (120.0, 100.0));
    let mut flame = FlameView::default();
    flame.sync(&report, &[], 0);
    assert_eq!(flame.icicle.nodes().len(), 2);
    // The same profile and findings: nothing made again (the zoom kept).
    flame.icicle.zoom_to(Some(1));
    flame.sync(&report, &[], 0);
    assert_eq!(flame.icicle.focus(), Some(1));
}
