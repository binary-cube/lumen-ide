#[test]
fn finds_deprecated_properties() {
    let found = super::find(".a { clip: rect(0 0 0 0); display: flex; page-break-after: always }");
    let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["clip", "page-break-after"]);
}
