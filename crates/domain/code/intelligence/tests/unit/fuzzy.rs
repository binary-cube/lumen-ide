use super::*;

#[test]
fn prefix_and_camel_case_win() {
    let get = fuzzy_score("get", "getName").unwrap();
    let mid = fuzzy_score("get", "targetName").unwrap();
    assert!(get > mid);
    assert!(fuzzy_score("gn", "getName").unwrap() > fuzzy_score("gn", "signal").unwrap_or(0));
    assert_eq!(fuzzy_score("xyz", "getName"), None);
    assert_eq!(fuzzy_score("", "orice"), Some(0));
}
