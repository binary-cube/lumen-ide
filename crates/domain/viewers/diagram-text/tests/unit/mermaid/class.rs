use super::super::{Diagram, parse as parse_any};
use super::*;

fn diagram(source: &str) -> ClassDiagram {
    match parse_any(source).expect("parsed").diagram {
        Diagram::Class(c) => c,
        other => panic!("{other:?}"),
    }
}

#[test]
fn classes_members_and_annotations() {
    let d = diagram(
        "classDiagram\n  class Animal {\n    <<abstract>>\n    +String name\n    +int age$\n    +makeSound()* void\n    +List~int~ ids\n  }\n\
         class Square~Shape~\n  Duck : +swim()\n  Duck : +String beak\n  <<interface>> Flyer\n  class Bank[\"Big Bank\"]",
    );
    let animal = &d.classes[0];
    assert_eq!(animal.annotation.as_deref(), Some("abstract"));
    assert_eq!(animal.attributes.len(), 3);
    assert!(animal.attributes[1].is_static);
    assert_eq!(animal.attributes[2].text, "+List<int> ids");
    assert!(animal.methods[0].is_abstract);
    assert_eq!(d.classes[1].label, "Square<Shape>");
    let duck = d.classes.iter().find(|c| c.id == "Duck").unwrap();
    assert_eq!((duck.methods.len(), duck.attributes.len()), (1, 1));
    assert!(
        d.classes.iter().any(|c| c.id == "Flyer" && c.annotation.as_deref() == Some("interface"))
    );
    assert!(d.classes.iter().any(|c| c.label == "Big Bank"));
}

#[test]
fn relations() {
    let d = diagram(
        "classDiagram\n  Animal <|-- Duck\n  Car *-- Wheel\n  Pond o-- Duck : has\n  A --> B\n  C ..> D\n  E ..|> F\n  G -- H\n  I .. J\n\
         Customer \"1\" --> \"*\" Ticket : buys\n  K <|--|> L",
    );
    let r = &d.relations;
    assert_eq!(r[0].from_mark, Mark::Inheritance);
    assert_eq!(r[1].from_mark, Mark::Composition);
    assert_eq!((r[2].from_mark, r[2].label.as_deref()), (Mark::Aggregation, Some("has")));
    assert_eq!(r[3].to_mark, Mark::Arrow);
    assert!(r[4].dashed && r[4].to_mark == Mark::Arrow);
    assert!(r[5].dashed && r[5].to_mark == Mark::Inheritance);
    assert_eq!((r[6].from_mark, r[6].to_mark, r[6].dashed), (Mark::None, Mark::None, false));
    assert!(r[7].dashed);
    assert_eq!((r[8].from_card.as_deref(), r[8].to_card.as_deref()), (Some("1"), Some("*")));
    assert_eq!((r[9].from_mark, r[9].to_mark), (Mark::Inheritance, Mark::Inheritance));
}

#[test]
fn notes_namespaces_and_errors() {
    let d = diagram(
        "classDiagram\n  note \"General\"\n  note for Duck \"Quacks\"\n  namespace Shapes {\n    class Triangle\n  }",
    );
    assert_eq!(d.notes.len(), 2);
    assert_eq!(d.notes[1].text, "Quacks");
    assert_eq!(d.namespaces[0].0, "Shapes");
    assert_eq!(d.namespaces[0].1.len(), 1);
    let e = parse_any("classDiagram\nclass A {\n+x\n").unwrap_err();
    assert_eq!(e.line, 2);
}
