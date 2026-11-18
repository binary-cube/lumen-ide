//! The profile's diagram: an edge as thick as the rows it carries (`log10`, 1–8 px), a card
//! per step laid out without overlaps both ways, each edge from a child into its parent, the
//! costliest branch's edges hot, a folded step's subtree a "+N" card, the tooltip's figures.

use super::*;
use database::profile::rules::Finding;

fn sample() -> std::sync::Arc<ProfileReport> {
    crate::profile::sample::report(crate::profile::sample::SQL)
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.min_x() < b.max_x() && b.min_x() < a.max_x() && a.min_y() < b.max_y() && b.min_y() < a.max_y()
}

#[test]
fn edges_grow_with_the_rows_they_carry() {
    assert_eq!(edge_width(0.0), 1.0);
    assert_eq!(edge_width(1.0), 1.0);
    assert!((edge_width(10.0) - 2.0).abs() < 1e-4);
    assert!((edge_width(18_399.0) - 5.26).abs() < 0.01);
    assert_eq!(edge_width(1e7), 8.0);
    assert_eq!(edge_width(1e12), 8.0, "at most 8 px");
    assert_eq!(edge_width(f64::NAN), 1.0);
    let mut last = 0.0;
    for rows in [1.0, 5.0, 50.0, 500.0, 5e4, 5e6] {
        let w = edge_width(rows);
        assert!(w >= last);
        last = w;
    }
}

#[test]
fn every_step_has_a_card_and_an_edge_into_its_parent() {
    let report = sample();
    for flow in [Flow::Down, Flow::Right] {
        let laid = layout(&report, &|_| true, flow);
        let n = report.profile.nodes.len();
        assert_eq!(laid.cards.len(), n);
        assert_eq!(laid.edges.len(), n - report.profile.roots.len());
        for i in 0..n {
            for j in i + 1..n {
                assert!(!overlap(laid.cards[i].rect, laid.cards[j].rect), "{flow:?}: {i} {j}");
            }
        }
        for (e, info) in laid.edges.iter().zip(&laid.info) {
            assert!(points_to_parent(e, flow));
            let (Item::Step(child), Item::Step(parent)) =
                (laid.cards[e.kind.0].item, laid.cards[e.kind.1].item)
            else {
                panic!("steps only");
            };
            assert_eq!(report.profile.node(child).parent, Some(parent));
            // As thick as its rows, labelled with them.
            let rows = rows_out(&report, child).unwrap();
            assert_eq!(info.width, edge_width(rows));
            assert_eq!(info.label.as_deref(), Some(figures::count(rows).as_str()));
            assert_eq!(info.hot, report.on_hot_path(child) && report.on_hot_path(parent));
        }
        // The costliest branch's edges are all hot.
        let hot = laid.info.iter().filter(|i| i.hot).count();
        assert_eq!(hot, report.hot_path.len() - 1);
        assert_eq!(laid.bounds.min_x(), 0.0);
        // Cards the same height (the same lines).
        assert!(laid.cards.iter().all(|c| c.rect.size == laid.cards[0].rect.size));
    }
}

#[test]
fn a_folded_step_shows_a_card_for_what_it_hides() {
    let report = sample();
    // The hash join folded: its four steps under it in one "+N" card.
    let join = report
        .profile
        .nodes
        .iter()
        .position(|n| n.name == "Parallel Hash Join")
        .map(|i| NodeId(i as u32))
        .unwrap();
    let laid = layout(&report, &|id| id != join, Flow::Down);
    let folded: Vec<&Card> =
        laid.cards.iter().filter(|c| matches!(c.item, Item::Folded { .. })).collect();
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0].item, Item::Folded { under: join, count: 3 });
    assert_eq!(laid.cards.len(), report.profile.nodes.len() - 3 + 1);
    assert_eq!(laid.card_of[report.profile.node(join).children[0].index()], usize::MAX);
    // Its edge is the folded card's, into the join.
    let edge = laid.edges.iter().find(|e| matches!(laid.cards[e.kind.0].item, Item::Folded { .. }));
    assert_eq!(edge.map(|e| laid.cards[e.kind.1].item), Some(Item::Step(join)));
}

#[test]
fn a_card_tip_says_the_exact_figures_and_its_finding() {
    let report = sample();
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let worst = tree::worst_of(&report, &findings);
    let scan = report.hot_path.last().copied().unwrap();
    let finding = worst[scan.index()].map(|f| findings[f]);
    let tip = card_tip(&report, scan, finding);
    assert!(tip.contains("505"), "{tip}");
    assert!(tip.contains("public.orders"), "{tip}");
    let title = crate::profile::analysis::texts(finding.unwrap()).title;
    assert!(tip.contains(&title), "{tip}");
}

#[test]
fn the_arrows_walk_the_tree_by_the_flow() {
    use ui_components::ViewerKey as K;
    let report = sample();
    let root = report.profile.roots[0];
    let child = report.profile.node(root).children[0];
    assert_eq!(GraphView::step_from(&report, root, K::Down, Flow::Down), Some(child));
    assert_eq!(GraphView::step_from(&report, child, K::Up, Flow::Down), Some(root));
    assert_eq!(GraphView::step_from(&report, root, K::Right, Flow::Right), Some(child));
    assert_eq!(GraphView::step_from(&report, child, K::Left, Flow::Right), Some(root));
    let join = report.profile.nodes.iter().position(|n| n.name == "Parallel Hash Join").unwrap();
    let kids = report.profile.nodes[join].children.clone();
    assert_eq!(GraphView::step_from(&report, kids[0], K::Right, Flow::Down), Some(kids[1]));
    assert_eq!(GraphView::step_from(&report, kids[1], K::Left, Flow::Down), Some(kids[0]));
    assert_eq!(GraphView::step_from(&report, kids[0], K::Left, Flow::Down), None);
}
