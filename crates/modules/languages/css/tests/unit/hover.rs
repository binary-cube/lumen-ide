use super::*;

#[test]
fn explains_properties() {
    let src = "a:hover { display: flex; clip: auto }";
    let info = hover(src, src.find("display").unwrap() + 2).unwrap();
    assert!(info.code.starts_with("display: ") && info.notes.is_empty(), "{info:?}");
    let clip = hover(src, src.find("clip").unwrap() + 1).unwrap();
    assert_eq!(clip.notes, [HoverNote::Deprecated]);
    assert!(hover(src, 0).is_none(), "selector");
}
