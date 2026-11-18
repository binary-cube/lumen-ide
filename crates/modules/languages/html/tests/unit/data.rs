#[test]
fn elements_attributes_and_values() {
    let html = super::get();
    assert!(html.elements.iter().any(|(t, f)| *t == "center" && f.contains('d')));
    assert!(html.global_attributes().iter().any(|(a, _)| *a == "class"));
    assert!(html.attributes("input").iter().any(|(a, _)| *a == "type"));
    assert!(html.values("input", "type").contains(&"email"));
}
