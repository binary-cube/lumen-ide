use super::*;

#[test]
fn types_front_matter_and_errors() {
    let p = parse("---\ntitle: Hello\n---\nflowchart LR\n  A --> B\n").expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("Hello"));
    assert!(matches!(p.diagram, Diagram::Flowchart(_)));
    assert_eq!(parse("%% only a comment\n").unwrap_err().kind, ErrorKind::Empty);
    let e = parse("\nzenuml\n  title x").unwrap_err();
    assert_eq!(e, Error::new(2, ErrorKind::Unsupported("zenuml".into())));
    assert!(matches!(parse("nonsense").unwrap_err().kind, ErrorKind::UnknownType(_)));
    assert_eq!(kind("%% c\nsankey-beta\n"), Some("sankey-beta".into()));
}

#[test]
fn accessibility_lines() {
    let p = parse("graph TD\naccTitle: The title\naccDescr {\n  multi\n  line\n}\nA-->B").unwrap();
    assert_eq!(p.meta.title.as_deref(), Some("The title"));
    assert_eq!(p.meta.description.as_deref(), Some("multi line"));
}

#[test]
fn labels_and_colors() {
    assert_eq!(label("\"a<br/>b #quot;c#quot;\""), "a\nb \"c\"");
    assert_eq!(color("#f9f"), Rgba::from_hex("#f9f"));
    assert_eq!(color("rgb(255, 0, 0)"), Some(Rgba::rgb(0xff0000)));
    assert_eq!(color("lightblue"), Some(Rgba::rgb(0xadd8e6)));
    let s = Style::parse("fill:#f9f,stroke:#333,stroke-width:4px,color:#fff,stroke-dasharray: 5 5");
    assert_eq!(s.stroke_width, Some(4.0));
    assert!(s.dashed && s.fill.is_some() && s.color.is_some());
}
