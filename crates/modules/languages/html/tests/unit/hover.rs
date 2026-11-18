use super::*;

#[test]
fn explains_elements_and_attributes() {
    let src = r#"<center><table bgcolor="red" class="x"></table></center>"#;
    let center = hover(src, 2).unwrap();
    assert_eq!(
        (center.code.as_str(), center.notes.as_slice()),
        ("<center>", &[HoverNote::Deprecated][..])
    );
    let bg = hover(src, src.find("bgcolor").unwrap() + 1).unwrap();
    assert_eq!(bg.code, "<table bgcolor>");
    assert_eq!(bg.notes, [HoverNote::Deprecated]);
    assert!(hover(src, src.find("red").unwrap() + 1).is_none(), "a value");
}
