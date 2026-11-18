use super::*;
use extractors::{Cell, Column, Extractor, Input, Kind, Options, TemplateExtractor, extract};

fn run(name: &str) -> String {
    let (_, text) = EXAMPLES.iter().find(|(n, _)| *n == name).unwrap();
    let e = TemplateExtractor::new("t", Path::new(name), text);
    assert_eq!(e.problem(), None, "{name}");
    let columns = vec![
        Column::new("id", Kind::Integer).keyed(),
        Column::new("name", Kind::Text),
        Column::new("active", Kind::Bool),
    ];
    let rows = vec![
        vec![Cell::Int(1), Cell::Text("O'Hara"), Cell::Bool(true)],
        vec![Cell::Int(2), Cell::Null, Cell::Bool(false)],
    ];
    extract(&e, Input::new("users", columns), Options::default(), &rows)
}

#[test]
fn every_example_is_read_without_an_error() {
    assert_eq!(EXAMPLES.len(), 5);
    for (name, text) in EXAMPLES {
        let e = TemplateExtractor::new("t", Path::new(name), text);
        assert_eq!(e.problem(), None, "{name}");
        assert_ne!(e.extension(), "txt", "{name} names its extension");
    }
}

#[test]
fn the_examples_write_what_they_say() {
    assert_eq!(
        run("SQL-Insert-Multirow.extractor"),
        "INSERT INTO \"users\" (\"id\", \"name\", \"active\")\nVALUES\n    (1, 'O''Hara', TRUE),\n    (2, NULL, FALSE);\n"
    );
    assert_eq!(
        run("Python-DataFrame.extractor"),
        "import pandas as pd\n\nusers = pd.DataFrame([\n    { 'id': 1, 'name': 'O\\'Hara', 'active': True },\n    { 'id': 2, 'name': None, 'active': False },\n])\n"
    );
    assert_eq!(
        run("PHP-Array.extractor"),
        "<?php\n\nreturn [\n    ['id' => 1, 'name' => 'O\\'Hara', 'active' => true],\n    ['id' => 2, 'name' => null, 'active' => false],\n];\n"
    );
    assert!(
        run("Laravel-Seeder.extractor")
            .contains("DB::table('users')->insert([\n            ['id' => 1,")
    );
    assert_eq!(
        run("Markdown-List.extractor"),
        "## users\n\n- **id**: 1, **name**: O'Hara, **active**: true\n- **id**: 2, **name**: _NULL_, **active**: false\n"
    );
}

#[test]
fn examples_are_written_once_and_never_over_the_user_s() {
    let dir = std::env::temp_dir().join(format!(
        "extractor-examples-{}-{:x}",
        std::process::id(),
        kernel::id::new_uuid()
    ));
    let written = write(&dir).unwrap();
    assert_eq!(written.len(), 5);
    std::fs::write(dir.join("PHP-Array.extractor"), "mine").unwrap();
    std::fs::remove_file(dir.join("Markdown-List.extractor")).unwrap();
    let again = write(&dir).unwrap();
    assert_eq!(again, vec![dir.join("Markdown-List.extractor")]);
    assert_eq!(std::fs::read_to_string(dir.join("PHP-Array.extractor")).unwrap(), "mine");
    let _ = std::fs::remove_dir_all(dir);
}
