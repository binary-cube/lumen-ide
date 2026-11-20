use super::*;

#[test]
fn the_search_takes_its_keys_and_asks_for_the_chosen_result() {
    let (mut open, mut chosen) = (Some(search::Search::default()), None);
    let mut part = SearchPart { search: &mut open, catalog: &[], chosen: &mut chosen };
    let reach = KeyReach::default();
    assert!(part.focusable() && part.keeps_tab());
    assert!(part.text("ab"));
    assert!(part.command("select_all"));
    assert!(part.search.as_ref().is_some_and(|s| s.all));
    assert!(part.key(ViewerKey::Left, reach), "← drops the choice of all");
    assert!(part.search.as_ref().is_some_and(|s| !s.all));
    assert!(part.key(ViewerKey::Backspace, reach));
    assert!(part.key(ViewerKey::Tab, reach), "Tab moves among the results");
    assert!(!part.key(ViewerKey::Delete, reach));
    assert!(part.key(ViewerKey::Enter, reach));
    assert_eq!(*part.chosen, Some(None), "no result to jump to");
    assert!(part.key(ViewerKey::Escape, reach));
    assert!(open.is_none() && chosen == Some(None));
}

#[test]
fn closed_nothing_takes_the_keys() {
    let (mut closed, mut chosen) = (None, None);
    let mut search = SearchPart { search: &mut closed, catalog: &[], chosen: &mut chosen };
    let (mut editing, mut relaid, mut finished) = (None, false, false);
    let mut code = EditPart { editing: &mut editing, relaid: &mut relaid, finished: &mut finished };
    let reach = KeyReach::default();
    assert!(!search.focusable() && !code.focusable());
    assert!(!search.key(ViewerKey::Enter, reach) && !search.text("x"));
    assert!(!code.key(ViewerKey::Escape, reach) && !code.text("x"));
    assert!(!relaid && !finished);
}
