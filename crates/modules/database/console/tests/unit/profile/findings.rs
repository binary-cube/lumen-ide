//! The findings' cards: laid out once per width (a Memo), their lines wrapped, the SQL
//! suggested in its own lines.

use super::*;

#[test]
fn the_cards_are_laid_out_once_per_width() {
    let report = crate::profile::sample::report(crate::profile::sample::SQL);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let mut text = TextSystem::new();
    let palette = ui_components::Palette::dark();
    let ds = Ctx::new(&palette, 2.0);
    let mut list = FindingsList::default();
    assert!(!list.laid_out());
    let first = list.cards(&mut text, &ds, 600.0, &findings, &report, 1).clone();
    assert_eq!(first.cards.len(), findings.len());
    assert!(first.cards.windows(2).all(|w| w[1].top > w[0].top + w[0].height - 1.0));
    // The same width and findings: the same layout, not made again.
    let again = list.cards(&mut text, &ds, 600.0, &findings, &report, 1).clone();
    assert_eq!(again, first);
    // Narrower: its lines wrapped again, taller.
    let narrow = list.cards(&mut text, &ds, 300.0, &findings, &report, 1).clone();
    assert!(narrow.height > first.height);
    let with_sql = narrow.cards.iter().find(|c| !c.sql.is_empty()).unwrap();
    assert!(with_sql.sql.len() > 1, "a long CREATE INDEX wraps: {:?}", with_sql.sql);
    assert!(with_sql.step.is_some());
}
