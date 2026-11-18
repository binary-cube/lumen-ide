#[test]
fn finds_deprecated_elements_and_attributes() {
    let found =
        super::find(r#"<center><div class="x">a</div><table bgcolor="red"></table></center>"#);
    let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["<center>", "bgcolor"]);
}
