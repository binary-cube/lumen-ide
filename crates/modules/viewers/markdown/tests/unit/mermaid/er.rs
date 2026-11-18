use super::super::tests::{scene_of, texts};

#[test]
fn entities_and_relations() {
    let scene = scene_of(
        "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER }|..|{ ADDRESS : uses\n\
         CUSTOMER {\n    string name\n    int id PK \"its number\"\n  }",
    );
    let t = texts(&scene);
    for word in [
        "CUSTOMER",
        "ORDER",
        "LINE-ITEM",
        "places",
        "contains",
        "uses",
        "string",
        "name",
        "PK",
        "its number",
    ] {
        assert!(t.contains(&word), "{word}: {t:?}");
    }
}
