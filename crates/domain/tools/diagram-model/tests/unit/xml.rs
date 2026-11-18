use super::*;

/// Numbers as JavaScript writes them.
#[test]
fn numbers_are_written_as_draw_io_writes_them() {
    assert_eq!(number(340.0), "340");
    assert_eq!(number(-0.2), "-0.2");
    assert_eq!(number(210.5), "210.5");
    assert_eq!(number(0.0), "0");
}

/// An attribute keeps the text it was read as while its value doesn't change; set anew,
/// it's escaped as draw.io escapes.
#[test]
fn an_attribute_keeps_its_text_until_it_changes() {
    let source = r#"<a v="x &#38; y" w="1"/>"#;
    let doc = roxmltree::Document::parse(source).unwrap();
    let mut el = Element::read(doc.root_element(), source, &mut 0, 10, 0).unwrap();
    assert_eq!(el.attr("v"), Some("x & y"));
    el.set_attr("v", "x & y");
    let mut out = String::new();
    el.write(&mut out, 0);
    assert_eq!(out, "<a v=\"x &#38; y\" w=\"1\" />\n", "as read");
    el.set_attr("v", "it's <1>\nnew");
    el.set_attr("z", "2");
    out.clear();
    el.write(&mut out, 0);
    assert_eq!(out, "<a v=\"it&#39;s &lt;1&gt;&#xa;new\" w=\"1\" z=\"2\" />\n");
}
