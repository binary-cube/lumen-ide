#[test]
fn properties_and_values() {
    let css = super::get();
    assert!(css.properties.iter().any(|p| p.name == "display" && p.flags.is_empty()));
    assert!(css.values("display").contains(&"flex"));
    assert!(css.values("display").contains(&"grid"));
}
