//! The profile's tree: only the columns its steps have (PostgreSQL's analyzed every figure,
//! SQLite's estimated plan its operations), the costliest branch marked, a finding's sign,
//! tint and outlined figure, the factor of an estimate far off (none under a LIMIT), the
//! columns left out when the room is short, big plans folded but for what matters.

use super::*;
use database::profile::testing::{ProfileBuilder, ran};
use ui_components::tree_table::CellValue;

fn sample() -> std::sync::Arc<ProfileReport> {
    crate::profile::sample::report(crate::profile::sample::SQL)
}

fn shown(report: &ProfileReport) -> Vec<&Finding> {
    report.findings.iter().collect()
}

#[test]
fn an_analyzed_profile_has_its_figures_as_columns() {
    let report = sample();
    let cols = columns_of(&report);
    for c in [
        Col::Operation,
        Col::SelfTime,
        Col::TotalTime,
        Col::Rows,
        Col::Loops,
        Col::Buffers,
        Col::Removed,
        Col::Spill,
        Col::Workers,
        Col::Cost,
        Col::Width,
        Col::Condition,
    ] {
        assert!(cols.contains(&c), "{c:?} in {cols:?}");
    }
    // In their fixed order.
    let order: Vec<Col> = Col::ORDER.into_iter().filter(|c| cols.contains(c)).collect();
    assert_eq!(cols, order);
    // An estimated SQLite plan: what it does and on what, nothing measured.
    let plan = database::plan::from_query_plan(&[
        (2, 0, "SCAN orders".into()),
        (3, 2, "SEARCH users USING INTEGER PRIMARY KEY (rowid=?)".into()),
    ]);
    let profile = database::profile::Profile::from_plan(
        &plan,
        database::profile::ProfileDepth::Estimated,
        database::profile::Engine::default(),
    );
    let report = database::profile::derive(profile);
    assert_eq!(columns_of(&report), [Col::Operation]);
}

#[test]
fn the_costliest_branch_and_the_problems_stand_out() {
    let report = sample();
    let findings = shown(&report);
    let table = table(&report, &columns_of(&report), &findings);
    assert_eq!(table.rows.len(), report.profile.nodes.len());
    // The hot path marked, down to the orders' scan.
    for id in &report.hot_path {
        assert!(table.rows[table.row_of[id.index()]].marked);
    }
    let scan = report.hot_path.last().copied().unwrap();
    let row = &table.rows[table.row_of[scan.index()]];
    let worst = findings
        .iter()
        .filter(|f| f.is_problem() && f.nodes.contains(&scan))
        .max_by(|a, b| (a.severity, a.impact).partial_cmp(&(b.severity, b.impact)).unwrap())
        .unwrap();
    let ink = Ink::Severity(level(worst.severity));
    assert_eq!(row.sign.map(|s| s.1), Some(ink));
    assert_eq!(row.tint, Some(ink));
    assert!(row.sign_tip.as_deref().unwrap().contains(&analysis::texts(worst).title));
    // The figure it's about outlined.
    let at = table.cols.iter().position(|c| *c == culprit(worst.rule)).unwrap();
    assert_eq!(row.cells[at].outline, Some(ink));
    // Its own time as a heat bar with its share written.
    let own = table.cols.iter().position(|c| *c == Col::SelfTime).unwrap();
    match &row.cells[own].value {
        CellValue::Bar { fraction, ink, label } => {
            assert!(*fraction > 0.5, "{fraction}");
            assert_eq!(*ink, Ink::heat(f64::from(*fraction)));
            assert!(label.contains("505"), "{label}");
        }
        other => panic!("{other:?}"),
    }
    // Rows: the factor in a pill when ×10 off.
    let rows = table.cols.iter().position(|c| *c == Col::Rows).unwrap();
    match &row.cells[rows].value {
        CellValue::Text(spans) => {
            assert!(
                matches!(spans[0].style, SpanStyle::Pill(Ink::Severity(Level::Warning))),
                "{spans:?}"
            );
            assert!(spans[0].text.contains('↑'));
        }
        other => panic!("{other:?}"),
    }
    // The top sort gives 50 of 1 180 planned under its LIMIT: no factor said.
    let sort = report.order[1];
    match &table.rows[table.row_of[sort.index()]].cells[rows].value {
        CellValue::Text(spans) => {
            assert!(!spans.iter().any(|s| matches!(s.style, SpanStyle::Pill(_))))
        }
        other => panic!("{other:?}"),
    }
    // A spill: the disk's icon and its size.
    let spill = table.cols.iter().position(|c| *c == Col::Spill).unwrap();
    let spilled = table.rows.iter().find(|r| r.cells[spill].plain() != "").unwrap();
    assert_eq!(spilled.cells[spill].lead.map(|l| l.0), Some("hard-drive"));
    // The workers launched short of those planned, in the warning's color.
    let workers = table.cols.iter().position(|c| *c == Col::Workers).unwrap();
    let gather = table.rows.iter().find(|r| r.cells[workers].plain() == "2/4").unwrap();
    assert!(
        matches!(&gather.cells[workers].value, CellValue::Text(s) if s[0].style == SpanStyle::Ink(Ink::Severity(Level::Warning)))
    );
    // Every figure's tooltip says it exactly.
    assert!(
        row.cells[own].tip.as_deref().unwrap().contains(&crate::profile::figures::ms_exact(505.3))
    );
}

#[test]
fn columns_are_left_out_by_their_importance() {
    let all = Col::ORDER.to_vec();
    assert_eq!(fitting(&all, 4000.0, 230.0), Fit { cols: all.clone(), compact: false });
    let narrow = fitting(&all, 560.0, 230.0).cols;
    assert!(narrow.contains(&Col::Operation) && narrow.contains(&Col::SelfTime));
    assert!(!narrow.contains(&Col::Width) && !narrow.contains(&Col::Condition));
    // Never fewer than the operation and its own time.
    assert_eq!(fitting(&all, 10.0, 230.0).cols.len(), 2);
}

/// The tree's words have the priority: the figures are narrowed, then left out, before an
/// operation's name is cut; what it asks for is kept within its bounds.
#[test]
fn the_tree_keeps_its_words_whole_first() {
    let cols = vec![Col::Operation, Col::SelfTime, Col::Rows];
    // Room for all at full width.
    assert_eq!(fitting(&cols, 700.0, 380.0), Fit { cols: cols.clone(), compact: false });
    // A little short: the figures narrowed, all kept.
    let fit = fitting(&cols, 380.0 + 116.0 + 164.0, 380.0);
    assert_eq!(fit, Fit { cols: cols.clone(), compact: true });
    // Shorter: the rows left out before the words are cut; the time at full width again.
    let fit = fitting(&cols, 552.0, 380.0);
    assert_eq!(fit.cols, vec![Col::Operation, Col::SelfTime]);
    assert!(!fit.compact);
    // What the words ask is bounded: a very deep tree doesn't push every figure out.
    assert_eq!(fitting(&cols, 760.0, 5000.0).cols, cols);
    assert!(Col::SelfTime.compact_width() < Col::SelfTime.width());
    assert_eq!(Col::Rows.compact_width(), Col::Rows.width(), "its pill and figures whole");
}

/// Past 300 steps, the small deep ones start folded, saying what they hide; the costliest
/// branch, the problems and what leads to them stay open.
#[test]
fn big_plans_start_folded_but_for_what_matters() {
    use database::profile::node::OpKind;
    let mut b =
        ProfileBuilder::new("postgres", database::profile::ProfileDepth::Actual { timing: true });
    b = b.child(OpKind::Append, "Append", |n| ran(n, 0.0, 1000.0, 1.0, 1.0));
    // A deep chain taking most of the time (the hot path).
    for _ in 0..20 {
        b = b.child(OpKind::Result, "Result", |n| ran(n, 0.0, 900.0, 1.0, 1.0));
    }
    for _ in 0..20 {
        b = b.up();
    }
    // 300 deep small branches beside it.
    for _ in 0..15 {
        for _ in 0..20 {
            b = b.child(OpKind::Result, "Small", |n| ran(n, 0.0, 0.001, 1.0, 1.0));
        }
        for _ in 0..20 {
            b = b.up();
        }
    }
    let report = database::profile::derive(b.build());
    assert!(report.profile.nodes.len() > BIG_PLAN);
    let t = table(&report, &columns_of(&report), &[]);
    for id in &report.hot_path {
        assert!(t.open[t.row_of[id.index()]], "the hot path stays open");
    }
    let small: Vec<usize> = (0..t.rows.len())
        .filter(|&r| report.derived(t.nodes[r]).depth >= DEEP && !report.on_hot_path(t.nodes[r]))
        .collect();
    assert!(!small.is_empty() && small.iter().all(|&r| !t.open[r]), "small deep steps folded");
    // What a folded row hides, said.
    let folded = small.iter().find(|&&r| t.rows[r].folded_note.is_some()).unwrap();
    assert!(t.rows[*folded].folded_note.as_deref().unwrap().starts_with('+'));
    // A small plan: all open.
    let report = sample();
    assert!(table(&report, &columns_of(&report), &[]).open.iter().all(|o| *o));
}
