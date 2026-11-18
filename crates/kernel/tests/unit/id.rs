use super::*;

#[test]
fn document_ids_are_unique() {
    let a = DocumentId::next();
    let b = DocumentId::next();
    assert_ne!(a, b);
}

#[test]
fn static_and_owned_ids_compare_equal() {
    static PHP: LanguageId = LanguageId::new_static("php");
    assert_eq!(PHP, LanguageId::new("php"));
    assert_eq!(ModuleId::new_static("lang.php").to_string(), "lang.php");
}

#[test]
fn every_project_opening_is_another_identity() {
    let (a, b) = (ProjectId::new(), ProjectId::new());
    assert_ne!(a, b, "two openings, even of the same folder, never collide");
    assert_ne!(ProjectId::default(), ProjectId::default());
    // A UUID v4, shown as one.
    let shown = a.to_string();
    assert_eq!(shown.len(), 36);
    assert_eq!(shown.as_bytes()[14], b'4');
}

#[test]
fn a_uuid_reads_back_from_its_text() {
    let id = new_uuid();
    let text = uuid_text(id);
    assert_eq!(parse_uuid(&text), Some(id));
    assert_eq!(parse_uuid(&text.replace('-', "")), Some(id), "the simple form too");
    assert_eq!(parse_uuid("not-a-uuid"), None);
}

#[test]
fn a_derived_uuid_is_stable_per_pair_and_never_a_random_one() {
    let parent = new_uuid();
    let shop = derived_uuid(parent, "shop");
    assert_eq!(shop, derived_uuid(parent, "shop"), "the same pair, the same identity");
    assert_ne!(shop, derived_uuid(parent, "analytics"));
    assert_ne!(shop, derived_uuid(new_uuid(), "shop"));
    assert_ne!(shop, parent);
    let uuid = uuid::Uuid::from_u128(shop);
    assert_eq!(uuid.get_version_num(), 8);
    assert_eq!(uuid.get_variant(), uuid::Variant::RFC4122);
    assert_eq!(parse_uuid(&uuid_text(shop)), Some(shop));
    // Fixed: found again after a restart.
    assert_eq!(uuid_text(derived_uuid(0, "postgres")), uuid_text(derived_uuid(0, "postgres")));
}
