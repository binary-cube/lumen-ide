use super::super::{Diagram, parse as parse_any};

#[test]
fn columns_spans_nesting_arrows() {
    let p = parse_any("block-beta\n  columns 3\n  a b c\n  d[\"A wide one\"]:2 e\n  space\n  block:group1:2\n    columns 2\n    f g\n  end\n  a --> b\n  e -- \"label\" --> f").expect("parsed");
    let Diagram::Block(d) = p.diagram else { panic!() };
    assert_eq!(d.columns, Some(3));
    let wide = d.blocks.iter().find(|b| b.id == "d").expect("d");
    assert_eq!((wide.label.as_str(), wide.span), ("A wide one", 2));
    assert!(d.blocks.iter().any(|b| b.space));
    let group = d.blocks.iter().find(|b| b.id == "group1").expect("group");
    assert_eq!((group.span, group.columns, group.children.len()), (2, Some(2), 2));
    assert_eq!(d.edges.len(), 2);
    assert_eq!(d.edges[1].2.as_deref(), Some("label"));
    assert!(!d.blocks.iter().any(|b| b.id.contains('"')));
}
