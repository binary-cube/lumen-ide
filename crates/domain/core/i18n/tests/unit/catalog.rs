use super::*;

#[test]
fn sections_comments_escapes() {
    let src = "# a comment\ntop = \"a\"\n[status]\nerrors = \"{n} \\\"e\\\"\\n\" # after\n\n[a.b]\nx=\"\\u0103\"\n";
    assert_eq!(
        parse_catalog(src),
        [
            ("top".to_owned(), "a".to_owned()),
            ("status.errors".to_owned(), "{n} \"e\"\n".to_owned()),
            ("a.b.x".to_owned(), "ă".to_owned()),
        ]
    );
}

/// The catalogs built in, read with their texts borrowed: the same keys and texts as read
/// whole, most of them borrowed (only the escaped ones copied).
#[test]
fn the_built_in_catalogs_borrow_their_texts() {
    for source in [include_str!("../../locales/ro.toml"), include_str!("../../locales/en.toml")] {
        let owned = parse_catalog(source);
        let borrowed = parse_catalog_static(source);
        assert_eq!(owned.len(), borrowed.len());
        for ((k1, v1), (k2, v2)) in owned.iter().zip(&borrowed) {
            assert_eq!((k1, v1.as_str()), (k2, v2.as_ref()));
        }
        let copied =
            borrowed.iter().filter(|(_, v)| matches!(v, std::borrow::Cow::Owned(_))).count();
        assert!(copied * 10 < borrowed.len(), "{copied} of {} copied", borrowed.len());
    }
}
