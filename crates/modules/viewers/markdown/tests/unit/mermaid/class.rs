use super::super::tests::{scene_of, texts};

#[test]
fn cards_relations_and_notes() {
    let scene = scene_of(
        "classDiagram\n  direction RL\n  class Animal {\n    <<abstract>>\n    +String name\n    +eat()* void\n  }\n\
         Animal <|-- Duck\n  Animal <|-- Fish\n  Duck *-- Egg : lays\n  Pond o-- Duck\n  Duck ..> Water\n\
         Customer \"1\" --> \"*\" Ticket\n  note for Duck \"Can fly\"\n  Duck : +swim()$",
    );
    let t = texts(&scene);
    for word in [
        "«abstract»",
        "Animal",
        "+String name",
        "+eat() void",
        "lays",
        "1",
        "*",
        "Can fly",
        "+swim()",
    ] {
        assert!(t.contains(&word), "{word}: {t:?}");
    }
}
