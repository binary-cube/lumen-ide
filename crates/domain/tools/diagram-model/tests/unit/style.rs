use super::*;

/// Read and written back the same: bare names, empty values, the final `;` or not.
#[test]
fn a_style_is_written_back_as_read() {
    for text in [
        "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;",
        "ellipse;whiteSpace=wrap;html=1;",
        "group",
        "text;html=1;points=[];align=center",
        "edgeLabel;html=1;resizable=0;points=[];",
        "",
    ] {
        assert_eq!(Style::parse(text).to_string(), text);
    }
}

/// Keys read, set in their place or at the end, taken away; the shape from `shape` or the
/// first bare name.
#[test]
fn keys_are_read_and_changed_in_place() {
    let mut style = Style::parse("ellipse;fillColor=#fff;rounded=1;fontSize=14;");
    assert_eq!(style.shape(), Some("ellipse"));
    assert!(style.has_name("ellipse") && style.flag("rounded"));
    assert_eq!(style.number("fontSize"), Some(14.0));
    style.set("fillColor", "#000");
    style.set("dashed", "1");
    style.remove("rounded");
    assert_eq!(style.to_string(), "ellipse;fillColor=#000;fontSize=14;dashed=1;");
    assert_eq!(Style::parse("shape=cylinder3;size=15").shape(), Some("cylinder3"));
    assert_eq!(Style::parse("rounded=0;").shape(), None, "draw.io's default");
    let mut new = Style::default();
    new.set("rounded", "0");
    assert_eq!(new.to_string(), "rounded=0;", "a new style ends as draw.io's");
}
