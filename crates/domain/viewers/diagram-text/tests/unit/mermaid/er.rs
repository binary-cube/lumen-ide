use super::super::{Diagram, parse as parse_any};
use super::*;

fn diagram(source: &str) -> ErDiagram {
    match parse_any(source).expect("parsed").diagram {
        Diagram::Er(e) => e,
        other => panic!("{other:?}"),
    }
}

#[test]
fn entities_attributes_and_relations() {
    let d = diagram(
        "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER }|..|{ DELIVERY-ADDRESS : uses\n\
         PERSON |o--o| CAR : drives\n  CUSTOMER {\n    string name\n    string custNumber PK \"the number\"\n    int age FK, UK\n  }\n  p[\"Person\"]",
    );
    assert_eq!(d.relations.len(), 4);
    let r = &d.relations[0];
    assert_eq!(
        (r.from_card, r.to_card, r.identifying),
        (Cardinality::ExactlyOne, Cardinality::ZeroOrMore, true)
    );
    assert_eq!(r.label, "places");
    assert_eq!(d.relations[1].to_card, Cardinality::OneOrMore);
    assert!(!d.relations[2].identifying);
    assert_eq!(d.relations[2].from_card, Cardinality::OneOrMore);
    assert_eq!(
        (d.relations[3].from_card, d.relations[3].to_card),
        (Cardinality::ZeroOrOne, Cardinality::ZeroOrOne)
    );
    let c = d.entities.iter().find(|e| e.id == "CUSTOMER").unwrap();
    assert_eq!(c.attributes.len(), 3);
    assert_eq!(c.attributes[1].keys, vec!["PK"]);
    assert_eq!(c.attributes[1].comment.as_deref(), Some("the number"));
    assert_eq!(c.attributes[2].keys, vec!["FK", "UK"]);
    assert!(d.entities.iter().any(|e| e.label == "Person"));
}
