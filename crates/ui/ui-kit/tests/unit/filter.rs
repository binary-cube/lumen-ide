use super::*;

#[test]
fn searches_ignore_case_and_diacritics() {
    assert_eq!(fold("Setări ȘI Țară"), "setari si tara");
    assert_eq!(fold("Proiecte"), "proiecte");
    // One character for each: positions found in the folded text are the text's.
    assert_eq!(fold("ÎÉ").chars().count(), 2);
}

#[test]
fn a_list_filter_finds_its_text_anywhere_in_an_item() {
    let filter = ListFilter::new("  setari ");
    assert!(filter.matches("Setări generale"));
    assert!(filter.matches("Deschide SETARI"));
    assert!(!filter.matches("Proiecte"));
    assert!(filter.matches_any(["Proiecte", "Setări"]));
    assert!(!filter.matches_any(["Proiecte", "Fișiere"]));
}

#[test]
fn an_empty_filter_lets_everything_through() {
    let filter = ListFilter::new("   ");
    assert!(filter.is_empty());
    assert!(filter.matches("anything"));
    assert!(filter.matches_any([]));
}
