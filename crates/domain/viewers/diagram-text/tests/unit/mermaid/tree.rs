use super::super::{Diagram, parse as parse_any};

#[test]
fn kanban_columns_and_cards() {
    let p = parse_any("kanban\n  Todo\n    [Create Documentation]\n    docs[Create Blog about the new diagram]\n  [In progress]\n    id6[Create renderer]\n  id9[Ready for deploy]\n    id8[Design grammar]@{ assigned: 'knsv' }").expect("parsed");
    let Diagram::Kanban(k) = p.diagram else { panic!() };
    assert_eq!(k.roots.len(), 3);
    assert_eq!(k.nodes[k.roots[1]].text, "In progress");
    let card = &k.nodes[k.nodes[k.roots[2]].children[0]];
    assert_eq!((card.text.as_str(), card.meta[0].1.as_str()), ("Design grammar", "knsv"));
}

#[test]
fn treemap_sections_and_values() {
    let p = parse_any("treemap-beta\n\"Section 1\"\n    \"Leaf 1.1\": 12\n    \"Section 1.2\"\n      \"Leaf 1.2.1\": 12\n\"Section 2\"\n    \"Leaf 2.1\": 20\n    \"Leaf 2.2\": 25").expect("parsed");
    let Diagram::Treemap(t) = p.diagram else { panic!() };
    assert_eq!(t.roots.len(), 2);
    assert_eq!(t.nodes[1].value, Some(12.0));
    assert_eq!(t.nodes[1].text, "Leaf 1.1");
}
