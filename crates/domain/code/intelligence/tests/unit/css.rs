use super::*;

#[test]
fn css_names_are_sure_in_stylesheets_and_attributes() {
    let stylesheet = CssReferences { stylesheet: true, ..Default::default() };
    let markup = CssReferences {
        class_attributes: &["class=\"", "class='"],
        id_attributes: &["id=\"", "id='"],
        ..Default::default()
    };
    let script =
        CssReferences { class_calls: &["classList.add("], script: true, ..Default::default() };
    let class = SymbolKind::CssClass;
    let css = ".card { } #card { }";
    let id = css.rfind("card").unwrap();
    assert_eq!(css_match(css, &(1..5), class, Some(&stylesheet)), PlaceMatch::Sure);
    assert_eq!(
        css_match(css, &(id..id + 4), class, Some(&stylesheet)),
        PlaceMatch::Other,
        "#card isn't the class"
    );
    let html = r#"<div class="box card">card</div>"#;
    let in_class = html.find("card").unwrap();
    let in_text = html.rfind("card").unwrap();
    assert_eq!(css_match(html, &(in_class..in_class + 4), class, Some(&markup)), PlaceMatch::Sure);
    assert_eq!(css_match(html, &(in_text..in_text + 4), class, Some(&markup)), PlaceMatch::Other);
    let js = "el.classList.add('card'); const name = 'card';";
    let added = js.find("card").unwrap();
    let other = js.rfind("card").unwrap();
    assert_eq!(css_match(js, &(added..added + 4), class, Some(&script)), PlaceMatch::Sure);
    assert_eq!(css_match(js, &(other..other + 4), class, Some(&script)), PlaceMatch::Maybe);
    // A file no language claims: only a string may be one.
    assert_eq!(css_match(js, &(other..other + 4), class, None), PlaceMatch::Maybe);
}
