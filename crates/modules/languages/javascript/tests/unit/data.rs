#[test]
fn globals_and_members() {
    let js = super::get();
    assert!(js.globals.iter().any(|(g, _)| *g == "Array"));
    assert_eq!(js.owner("document"), Some("Document"));
    assert!(js.members("Document").iter().any(|(m, _)| *m == "getElementById"));
    assert!(js.members("Array").iter().any(|(m, _)| *m == "at"));
    assert_eq!(
        js.signature("Document", "querySelector"),
        Some("(selectors: string): Element | null")
    );
    assert!(js.signature("", "parseInt").is_some_and(|s| s.contains("radix")));
    assert_eq!(js.any_signature("addEventListener").map(|(o, _)| o), Some("EventTarget"));
}
