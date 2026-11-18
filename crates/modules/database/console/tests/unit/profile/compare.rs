//! Two profiles compared: the same query before and after an index — its steps aligned (the
//! scan changed, not removed and added), better in blue with an arrow down, the headline,
//! the findings resolved; as a view, chosen from Compare ▾, swapped, closed.

use super::*;

fn sides() -> (Side, Side) {
    let text = crate::profile::sample::SQL;
    let before = crate::profile::sample::report(text);
    let (after, _) = crate::profile::sample::indexed(text);
    (
        Side { report: before, label: "10:00:23 · 845.1 ms".into() },
        Side { report: after, label: "10:00:24 · 12.4 ms".into() },
    )
}

#[test]
fn before_and_after_an_index() {
    let (before, after) = sides();
    let view = CompareView::new(before, after, true);
    let d = &view.diff;
    assert!(d.shape_changed);
    assert_eq!(d.measure, Some(Measure::Time));
    // The orders' scan: changed, its words before and after.
    let row = d
        .rows
        .iter()
        .position(|r| {
            r.change == Change::Changed
                && r.after.is_some_and(|a| {
                    view.after.report.profile.node(a).index.as_deref()
                        == Some("idx_orders_status_created")
                })
        })
        .expect("the scan changed");
    let step = view.built.rows[row].cells[0].plain();
    assert!(
        step.contains("Parallel Seq Scan") && step.contains("→") && step.contains("Index Scan"),
        "{step}"
    );
    assert_eq!(view.built.rows[row].sign.map(|s| s.0), Some("compare"));
    // Its change: better, blue, an arrow down.
    let change = &view.built.rows[row].cells[3];
    assert_eq!(change.lead, Some(("arrow-down", Ink::Severity(Level::Info))));
    assert!(change.plain().starts_with('−'), "{}", change.plain());
    // The headline.
    let head = view.headline();
    assert!(
        head.contains(&figures::ms(845.06))
            && head.contains(&figures::ms(12.38))
            && head.contains("98"),
        "{head}"
    );
    // The selective scan and the spill are resolved; nothing is lost of either side.
    let resolved: Vec<&str> =
        d.findings.resolved.iter().map(|i| view.before.report.findings[*i].rule).collect();
    assert!(resolved.contains(&"seq-scan-selective"), "{resolved:?}");
    assert!(resolved.contains(&"spill-to-disk"), "{resolved:?}");
    let n = |r: &ProfileReport| r.profile.nodes.len();
    assert_eq!(d.rows.iter().filter(|r| r.before.is_some()).count(), n(&view.before.report));
    assert_eq!(d.rows.iter().filter(|r| r.after.is_some()).count(), n(&view.after.report));
    // Swapped, worse.
    let back = view.swapped();
    assert_eq!(back.diff.statement.time.trend(true), Trend::Worse);
    assert!(!back.shown_after);
}

#[test]
fn changes_are_written_with_their_sign() {
    assert_eq!(
        change_text(&Delta::new(Some(100.0), Some(50.0))),
        format!("−{}", figures::share_exact(0.5))
    );
    assert_eq!(
        change_text(&Delta::new(Some(100.0), Some(130.0))),
        format!("+{}", figures::share_exact(0.3))
    );
    assert_eq!(change_text(&Delta::new(Some(100.0), Some(101.0))), "≈");
    assert_eq!(trend_icon(Trend::Worse), Some("arrow-up"));
    assert_eq!(trend_ink(Trend::Even), None);
    assert_eq!(
        side_label("2026-10-31 10:00:23.698", Some(845.06)),
        format!("10:00:23 · {}", figures::ms(845.06))
    );
}
