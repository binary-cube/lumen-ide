use super::*;

#[test]
fn versions_differ() {
    let names = |major| -> Vec<&str> { for_version(major).iter().map(|e| e.name).collect() };
    let (two, four) = (names(2), names(4));
    assert!(two.contains(&"hx-get") && four.contains(&"hx-get"));
    assert!(two.contains(&"hx-params") && !four.contains(&"hx-params"));
    assert!(four.contains(&"hx-action") && !two.contains(&"hx-action"));
    assert!(newest() >= 4);
}
