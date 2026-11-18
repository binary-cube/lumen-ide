use super::*;

#[test]
fn names_and_values() {
    let src = "<button class=\"x\" hx-sw";
    assert_eq!(
        code_intelligence::attribute_name_at(src, src.len(), &['-', ':']).map(|r| &src[r]),
        Some("hx-sw")
    );
    assert!(
        code_intelligence::attribute_name_at("<p>hx-sw", 8, &['-', ':']).is_none(),
        "not in a tag"
    );
    let src = "<div hx-swap:inherited=\"outerHTML sw";
    let content = quoted_at(src, src.len()).unwrap();
    assert_eq!(attribute_of(src, &content), Some("hx-swap"));
    assert!(attributes::values("hx-swap", 4).contains(&"outerMorph"));
    assert!(!attributes::values("hx-swap", 2).contains(&"outerMorph"));
    let support = Support { major: 4, entries: data::for_version(4) };
    assert!(support.attribute("hx-action").is_some() && support.attribute("hx-params").is_none());
}

#[test]
fn finds_the_version() {
    let detect = |file: &'static str, text: &'static str| {
        let read = move |p: &std::path::Path| p.ends_with(file).then(|| text.to_owned());
        let root = std::path::Path::new("/p");
        let project = Project {
            root: Some(root),
            read: &read,
            list: &|_| Vec::new(),
            modified: &|_| None,
            levels: &[],
            languages: &[],
        };
        version(&project)
    };
    let four = detect("package.json", r#"{"dependencies": {"htmx.org": "^4.0.0"}}"#);
    assert_eq!(four, Some(("package.json".into(), 4)));
    let cdn = detect("index.html", r#"<script src="https://unpkg.com/htmx.org@2.0.4"></script>"#);
    assert_eq!(cdn, Some(("index.html".into(), 2)));
    let bare = detect("index.html", r#"<script src="https://unpkg.com/htmx.org"></script>"#);
    assert_eq!(bare.map(|(_, m)| m), Some(2), "unpinned: the CDN serves htmx 2");
}
