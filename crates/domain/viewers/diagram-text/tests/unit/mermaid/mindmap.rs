use super::super::{Diagram, parse as parse_any};
use super::*;

#[test]
fn a_tree_by_indentation_with_shapes() {
    let p = parse_any("mindmap\n  root((mindmap))\n    Origins\n      Long history\n      ::icon(fa fa-book)\n    Research[On effectiveness]\n      id)Cloud(\n    Tools{{Pen}}\n      id))Bang((").expect("parsed");
    let Diagram::Mindmap(m) = p.diagram else { panic!() };
    assert_eq!(m.nodes.len(), 7);
    assert_eq!((m.nodes[0].text.as_str(), m.nodes[0].shape), ("mindmap", Shape::Circle));
    assert_eq!(m.nodes[0].children, vec![1, 3, 5]);
    assert_eq!(m.nodes[2].parent, Some(1));
    assert_eq!(m.nodes[3].shape, Shape::Square);
    assert_eq!(m.nodes[4].shape, Shape::Cloud);
    assert_eq!(m.nodes[5].shape, Shape::Hexagon);
    assert_eq!(m.nodes[6].shape, Shape::Bang);
}
