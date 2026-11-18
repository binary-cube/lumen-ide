//! A body's kind, its text made readable, its extension, a size.

use super::*;

#[test]
fn a_body_s_kind_comes_from_its_media_type() {
    assert_eq!(kind(Some("application/json")), Kind::Json);
    assert_eq!(kind(Some("application/problem+json")), Kind::Json);
    assert_eq!(kind(Some("text/html")), Kind::Html);
    assert_eq!(kind(Some("application/xml")), Kind::Xml);
    assert_eq!(kind(Some("application/x-yaml")), Kind::Yaml);
    assert_eq!(kind(Some("image/png")), Kind::Image);
    assert_eq!(kind(Some("application/octet-stream")), Kind::Binary);
    assert_eq!(kind(None), Kind::Text);
    assert_eq!(extension(Some("image/jpeg")), "jpg");
    assert_eq!(extension(Some("text/plain")), "txt");
}

#[test]
fn json_and_xml_are_made_readable() {
    assert_eq!(readable(Kind::Json, br#"{"a":[1,2]}"#), "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
    assert_eq!(readable(Kind::Json, b"not json"), "not json", "left as it was");
    assert_eq!(
        indent_xml("<?xml version=\"1.0\"?><a><b>text</b><c/></a>"),
        "<?xml version=\"1.0\"?>\n<a>\n  <b>text</b>\n  <c/>\n</a>"
    );
    let laid_out = "<a>\n  <b/>\n</a>\n";
    assert_eq!(indent_xml(laid_out), laid_out, "already over lines");
    assert_eq!(size(512), "512 B");
    assert_eq!(size(88_023), "88.02 kB");
    assert_eq!(size(3_500_000), "3.50 MB");
}
