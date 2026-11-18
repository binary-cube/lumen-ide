use super::super::{Diagram, parse as parse_any};

#[test]
fn csv_rows() {
    let p = parse_any("sankey-beta\n\nAgricultural 'waste',Bio-conversion,124.729\nBio-conversion,Liquid,0.597\n\"Bio-conversion\",\"Losses, other\",26.862\nBio-conversion,Solid,280.322").expect("parsed");
    let Diagram::Sankey(s) = p.diagram else { panic!() };
    assert_eq!(s.nodes.len(), 5);
    assert!(s.nodes.contains(&"Losses, other".to_owned()));
    assert_eq!(s.links[0], (0, 1, 124.729));
    assert!(parse_any("sankey-beta\na,b").is_err());
}
