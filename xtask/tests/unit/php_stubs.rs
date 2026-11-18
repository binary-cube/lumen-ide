use super::*;

#[test]
fn records_are_keyed_by_kind_and_names() {
    assert_eq!(key("f\tstrlen\tstring $s\tint\tcore").as_deref(), Some("f\tstrlen"));
    assert_eq!(key("df\tutf8_encode").as_deref(), Some("df\tutf8_encode"));
    assert_eq!(
        key("m\tDateTime\tformat\tpublic\tstring $f\tstring").as_deref(),
        Some("m\tdatetime\tformat")
    );
    assert_eq!(key("dm\tDateTime\tformat").as_deref(), Some("dm\tdatetime\tformat"));
    assert_eq!(
        key("p\tDateInterval\ty\tpublic\tint").as_deref(),
        Some("p\tdateinterval\ty"),
        "a property"
    );
    assert_eq!(
        key("c\tclass\tDateTime\t\tDateTimeInterface\tdate").as_deref(),
        Some("c\tdatetime")
    );
    assert_eq!(key("x\tsomething"), None);
}
