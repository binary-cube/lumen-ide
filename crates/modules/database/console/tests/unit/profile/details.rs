//! The details: a step's sections, only those with something in them (its conditions as
//! SQL, estimated against real, I/O, memory, workers, its engine's own, its problems); the
//! statement's own when none is chosen.

use super::*;

fn titles(sections: &[Section]) -> Vec<String> {
    sections.iter().map(|s| s.title.clone()).collect()
}

#[test]
fn a_step_s_details_have_only_what_it_has() {
    let report = crate::profile::sample::report(crate::profile::sample::SQL);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let scan = *report.hot_path.last().unwrap();
    let sections = sections(&report, Some(scan), &findings);
    let t = |k: &str| i18n::t(&format!("profile.view.details.{k}"));
    let names = titles(&sections);
    for k in ["operation", "object", "conditions", "estimated_actual", "io", "problems"] {
        assert!(names.contains(&t(k)), "{k} in {names:?}");
    }
    assert!(!names.contains(&t("wal")) && !names.contains(&t("output")), "nothing empty");
    // Its engine's own (an extra), by its key.
    let specific = sections.iter().find(|s| s.title.contains("PostgreSQL")).unwrap();
    assert!(matches!(&specific.lines[0], Line::Value { label, .. } if label == "Async Capable"));
    // Its conditions as SQL.
    let conditions = sections.iter().find(|s| s.title == t("conditions")).unwrap();
    assert!(
        matches!(&conditions.lines[0], Line::Value { code: true, label, .. } if label == "Filter")
    );
    // Its problems name their finding.
    let problems = sections.iter().find(|s| s.title == t("problems")).unwrap();
    for line in &problems.lines {
        let Line::Problem { finding, .. } = line else { panic!("{line:?}") };
        assert!(findings[*finding].nodes.contains(&scan));
    }
    // None chosen: the statement's (its engine, planning, JIT…).
    let statement = sections_of_statement(&report);
    let names = titles(&statement);
    assert!(names.contains(&t("statement")) && names.contains(&"JIT".to_owned()));
    let first = &statement[0].lines[0];
    assert!(matches!(first, Line::Value { value, .. } if value == "PostgreSQL 17.2"));
}

fn sections_of_statement(report: &ProfileReport) -> Vec<Section> {
    sections(report, None, &[])
}

#[test]
fn extras_are_shown_by_group_and_kind() {
    use database::profile::{Extra, ExtraValue, Quantity, Unit};
    assert_eq!(extra_value(&ExtraValue::Bool(true)).0, i18n::t("profile.view.details.yes"));
    let (shown, raw) = extra_value(&ExtraValue::Quantity(Quantity::new(2048.0, Unit::Bytes)));
    assert_eq!(shown, i18n::bytes(2048));
    assert!(raw.contains("2048"));
    let lines = extras(&[
        Extra::new("Functions", ExtraValue::Int(3)).in_group("JIT"),
        Extra::new(
            "Options",
            ExtraValue::Group(vec![Extra::new("Inlining", ExtraValue::Bool(false))]),
        ),
    ]);
    assert!(matches!(&lines[0], Line::Value { label, .. } if label == "JIT › Functions"));
    assert!(matches!(&lines[1], Line::Value { label, .. } if label == "Options › Inlining"));
}
