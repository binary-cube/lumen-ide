use super::*;

#[test]
fn knows_versions_and_signatures() {
    let stubs = get();
    assert_eq!(stubs.versions.first(), Some(&"5.6"));
    let f = stubs.function("str_contains").copied();
    assert!(f.is_some_and(|f| f.since == "8.0" && f.signature().contains("$needle")));
    assert_eq!(stubs.since("str_contains"), Some("8.0"));
    assert_eq!(stubs.since("strlen"), None, "always available");
    assert_eq!(stubs.removed("each"), Some("8.0"));
    assert_eq!(stubs.since("UnitEnum"), Some("8.1"));
    let found: Vec<_> = stubs.classes_with_prefix("ArrayOb").map(|c| c.name).collect();
    assert_eq!(found, ["ArrayObject"]);
    assert_eq!(stubs.extension("strlen"), None, "PHP's own");
    assert!(stubs.functions_with_prefix("array_fi").any(|f| f.name == "array_filter"));
}

#[test]
fn classes_know_their_members_and_ancestors() {
    let stubs = get();
    let date = stubs.class("DateTimeImmutable").copied().expect("a class");
    assert!(date.ancestors().any(|a| a == "DateTimeInterface"));
    let format = stubs.methods("DateTimeImmutable").iter().find(|m| m.name == "modify").copied();
    assert!(format.is_some_and(|m| !m.is_static()
        && m.is_public()
        && m.returns.contains("DateTimeImmutable")));
    let create =
        stubs.methods("\\datetimeimmutable").iter().find(|m| m.name == "createFromFormat").copied();
    assert!(create.is_some_and(|m| m.is_static()), "case and a leading \\ don't matter");
    assert!(stubs.methods("BackedEnum").iter().any(|m| m.name == "tryFrom" && m.is_static()));
    assert!(stubs.class_constants("DateTimeInterface").contains(&"ATOM"));
    let exception = stubs.class("RuntimeException").copied().expect("a class");
    assert_eq!(exception.parent, "Exception");
    assert!(stubs.methods("Exception").iter().any(|m| m.name == "getMessage"));
    assert!(stubs.methods("NoSuchClass").is_empty());
}
