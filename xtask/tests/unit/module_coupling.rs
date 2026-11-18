use super::*;

fn source(module: &str, file: &str, text: &str) -> Source {
    let module = format!("crates/modules/{module}");
    Source { path: format!("{module}/src/{file}"), module, text: text.to_owned() }
}

#[test]
fn modules_that_stay_apart_are_fine() {
    let sources = [
        source(
            "a/one",
            "lib.rs",
            r#"const X: CommandId = CommandId("one.open"); include_str!("../data/x");"#,
        ),
        source(
            "a/two",
            "lib.rs",
            r#"const Y: CommandId = CommandId("two.open"); depends_on: &[],"#,
        ),
    ];
    assert_eq!(violations(&sources), Vec::<String>::new());
}

#[test]
fn a_dependency_a_foreign_file_and_a_foreign_id_are_each_said() {
    let sources = [
        source(
            "a/one",
            "lib.rs",
            r#"const X: CommandId = CommandId("one.open"); pub const EXTENSION: &str = "view-one";"#,
        ),
        source("a/two", "lib.rs", r#"depends_on: &["one"],"#),
        source("a/three", "lib.rs", r#"include_str!("../../one/queries/x.scm")"#),
        source("a/four", "lib.rs", r#"run("one.open"); open("view-one");"#),
    ];
    let found = violations(&sources);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found[0].contains("names `one.open`") && found[1].contains("names `view-one`"));
    assert!(found[2].contains("reads another module's file crates/modules/a/one/queries/x.scm"));
    assert!(found[3].contains("depends_on"));
}
