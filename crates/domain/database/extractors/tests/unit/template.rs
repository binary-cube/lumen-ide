use super::*;
use crate::cell::{Cell, Column, Kind};
use crate::extractor::{Extraction, Options, extract};

fn columns() -> Vec<Column> {
    vec![
        Column::new("id", Kind::Integer).keyed().with_type("INTEGER"),
        Column::new("name", Kind::Text).with_type("TEXT"),
        Column::new("price", Kind::Decimal).with_type("NUMERIC"),
    ]
}

fn rows() -> Vec<Vec<Cell<'static>>> {
    vec![
        vec![Cell::Int(1), Cell::Text("Ann \"A\""), Cell::Other("2.50")],
        vec![Cell::Int(2), Cell::Text("O'Hara"), Cell::Null],
    ]
}

fn run(text: &str, rows: &[Vec<Cell<'_>>]) -> String {
    let e =
        TemplateExtractor::new("template:test", std::path::Path::new("/x/Test.extractor"), text);
    assert_eq!(e.error(), None, "{text}");
    extract(&e, Input::new("items", columns()), Options::default(), rows)
}

fn error(text: &str) -> TemplateError {
    let e = TemplateExtractor::new("t", std::path::Path::new("/x/T.extractor"), text);
    e.error().cloned().unwrap_or_else(|| panic!("no error in {text:?}"))
}

#[test]
fn loops_over_rows_and_columns_with_separators() {
    let t = "{{#rows}}{{#columns}}{{column.name}}={{value}}{{^last}}, {{/last}}{{/columns}}{{^last}}; {{/last}}{{/rows}}";
    assert_eq!(run(t, &rows()), "id=1, name=Ann \"A\", price=2.50; id=2, name=O'Hara, price=");
    assert_eq!(run(t, &[]), "");
}

#[test]
fn a_header_names_it_and_gives_its_extension() {
    let e = TemplateExtractor::new(
        "template:p",
        std::path::Path::new("/x/Py.extractor"),
        "---\nname: Python DataFrame\nextension: PY\n---\nx",
    );
    assert_eq!(e.name(), "Python DataFrame");
    assert_eq!(e.extension(), "py");
    assert_eq!(e.group(), crate::Group::Scripted);
    // Without one: the file's name, `txt`.
    let plain = TemplateExtractor::new("t", std::path::Path::new("/x/My List.extractor"), "x");
    assert_eq!((plain.name().as_str(), plain.extension()), ("My List", "txt"));
    assert_eq!(plain.origin(), Some(std::path::Path::new("/x/My List.extractor")));
}

#[test]
fn the_header_s_columns_and_the_rows_are_written_apart() {
    let t = "-- {{table}}\n{{#columns}}{{column.name|upper}}{{^last}}\t{{/last}}{{/columns}}\n{{#rows}}\n{{row.number}}. {{#columns}}{{value}}{{^last}}\t{{/last}}{{/columns}}\n{{/rows}}\n-- end\n";
    assert_eq!(
        run(t, &rows()),
        "-- items\nID\tNAME\tPRICE\n1. 1\tAnn \"A\"\t2.50\n2. 2\tO'Hara\t\n-- end\n"
    );
}

#[test]
fn conditions_on_nulls_numbers_keys_and_the_first() {
    let t = "{{#rows}}{{#columns}}{{#key}}*{{/key}}{{#null}}NULL{{/null}}{{^null}}{{#number}}#{{value}}{{/number}}{{^number}}'{{value}}'{{/number}}{{/null}}{{#first}}<{{/first}} {{/columns}}|{{/rows}}";
    assert_eq!(run(t, &rows()), "*#1< 'Ann \"A\"' #2.50 |*#2< 'O'Hara' NULL |");
    let firsts = "{{#rows}}{{#first}}[{{/first}}{{^first}},{{/first}}{{row.index}}{{#last}}]{{/last}}{{/rows}}";
    assert_eq!(run(firsts, &rows()), "[0,1]");
}

#[test]
fn filters_escape_for_each_language() {
    let one = vec![vec![Cell::Int(7), Cell::Text("<a & \"b\">|x,\ny"), Cell::Null]];
    let field = |filter: &str| {
        run(
            &format!(
                "{{{{#rows}}}}{{{{#columns}}}}{{{{#first}}}}{{{{/first}}}}{{{{^first}}}}{{{{^last}}}}{{{{value{filter}}}}}{{{{/last}}}}{{{{/first}}}}{{{{/columns}}}}{{{{/rows}}}}"
            ),
            &one,
        )
    };
    assert_eq!(field("|sql"), "'<a & \"b\">|x,\ny'");
    assert_eq!(field("|json"), "\"<a & \\\"b\\\">|x,\\ny\"");
    assert_eq!(field("|csv"), "\"<a & \"\"b\"\">|x,\ny\"");
    assert_eq!(field("|html"), "&lt;a &amp; &quot;b&quot;&gt;|x,\ny");
    assert_eq!(field("|xml"), "&lt;a &amp; &quot;b&quot;&gt;|x,\ny");
    assert_eq!(field("|md"), "<a & \"b\">\\|x,<br>y");
    assert_eq!(field("|upper"), "<A & \"B\">|X,\nY");
    let names = "{{table|sql}} {{#columns}}{{column.name|sql}}{{column.name|json}}{{column.index|json}}{{/columns}}";
    assert_eq!(run(names, &[]), "\"items\" \"id\"\"id\"0\"name\"\"name\"1\"price\"\"price\"2");
    let nulls =
        "{{#rows}}{{#columns}}[{{value|sql}}|{{value|json}}|{{value|csv}}]{{/columns}}{{/rows}}";
    assert_eq!(run(nulls, &rows()[1..]), "[2|2|2]['O''Hara'|\"O'Hara\"|O'Hara][NULL|null|]");
    let padded = "{{#columns}}[{{column.name|pad:6}}][{{column.name|pad:-6}}][{{column.type|lower|trim}}]{{/columns}}";
    assert_eq!(
        run(padded, &[]),
        "[id    ][    id][integer][name  ][  name][text][price ][ price][numeric]"
    );
}

#[test]
fn section_lines_alone_leave_no_blank_lines() {
    let t = "[\n{{#rows}}\n  {{row.number}}\n{{/rows}}\n]";
    assert_eq!(run(t, &rows()), "[\n  1\n  2\n]");
    // Indented tags alone on their lines, a comment, and an escaped brace.
    let t = "a\n  {{! a comment }}\n  {{#rows}}\n  - {{#columns}}{{value}}{{/columns}}\n  {{/rows}}\nb \\{{x}} \\{#y\n{# another, 'quoted' #}\nc";
    assert_eq!(run(t, &rows()[..1]), "a\n  - 1Ann \"A\"2.50\nb {{x}} {#y\nc");
    assert_eq!(error("x {# open").column, 3);
}

#[test]
fn rows_written_in_blocks_give_the_same_text() {
    let t = "{{#rows}}{{row.index}}{{#last}}!{{/last}}{{^last}},{{/last}}{{/rows}}";
    let e = TemplateExtractor::new("t", std::path::Path::new("/x/T.extractor"), t);
    let many: Vec<Vec<Cell<'_>>> =
        (0..10).map(|i| vec![Cell::Int(i), Cell::Null, Cell::Null]).collect();
    let whole = extract(&e, Input::new("t", columns()), Options::default(), &many);
    let mut x = Extraction::new(&e, Input::new("t", columns()), Options::default());
    let mut pieces = x.begin();
    for block in many.chunks(3) {
        pieces.push_str(&x.rows(block));
    }
    pieces.push_str(&x.finish());
    assert_eq!(pieces, whole);
    assert_eq!(whole, "0,1,2,3,4,5,6,7,8,9!");
}

#[test]
fn errors_say_where_they_are() {
    let e = error("ok\n  {{#rows}}{{value}");
    assert_eq!((e.line, e.column), (2, 12));
    let e = error("{{#rows}}\n{{#colums}}{{/colums}}{{/rows}}");
    assert_eq!((e.line, e.column), (2, 4));
    assert!(e.message.contains("colums") || e.message.starts_with("extract."), "{}", e.message);
    // A field or a filter unknown, a filter's argument wrong.
    let e = error("{{#rows}}{{#columns}}{{value|sqll}}{{/columns}}{{/rows}}");
    assert_eq!((e.line, e.column), (1, 30));
    assert_eq!(error("{{#rows}}{{#columns}}{{value|pad:x}}{{/columns}}{{/rows}}").column, 30);
    assert_eq!(error("{{nothing}}").column, 3);
    // Closed in the wrong order, never closed, closed without being opened.
    assert_eq!(error("{{#rows}}{{#columns}}{{/rows}}").column, 24);
    assert_eq!((error("x\n{{#rows}}").line, error("x\n{{#rows}}").column), (2, 1));
    assert_eq!(error("{{/rows}}").column, 3);
    // In the wrong place: a value outside a row's columns, rows twice or inside columns.
    assert_eq!(error("{{value}}").column, 3);
    assert_eq!(error("{{#columns}}{{value}}{{/columns}}").column, 15);
    assert_eq!(error("{{#columns}}{{row.index}}{{/columns}}").column, 15);
    assert_eq!(error("{{#rows}}{{/rows}}{{#rows}}{{/rows}}").column, 21);
    assert_eq!(error("{{#columns}}{{#rows}}{{/rows}}{{/columns}}").column, 15);
    assert_eq!(error("{{#first}}{{/first}}").column, 3);
    assert_eq!(error("{{^rows}}{{/rows}}").column, 3);
    assert_eq!(
        error("{{#rows}}{{#columns}}{{#columns}}{{/columns}}{{/columns}}{{/rows}}").column,
        24
    );
    // The header.
    assert_eq!(error("---\nname: x\n").line, 1);
    assert_eq!(error("---\ncolor: red\n---\n").line, 2);
    assert_eq!(error("---\nextension: p.y\n---\n").line, 2);
    // Too deep, too long.
    let deep = "{{#rows}}{{#columns}}".to_owned() + &"{{#null}}".repeat(20);
    assert!(error(&deep).message.contains("deep") || error(&deep).column > 1);
    assert_eq!(error(&"x".repeat(MAX_LEN + 1)).line, 1);
}

#[test]
fn a_template_with_an_error_writes_nothing_and_says_why() {
    let e = TemplateExtractor::new("t", std::path::Path::new("/x/T.extractor"), "{{#rows}}");
    assert!(e.problem().is_some());
    assert_eq!(extract(&e, Input::new("t", columns()), Options::default(), &rows()), "");
}

#[test]
fn nothing_outside_the_rows_is_reachable() {
    // No field reads anything but the rows given: an unknown name is an error, never a file.
    for name in ["file", "env.HOME", "../etc/passwd", "include", "system"] {
        assert!(
            TemplateExtractor::new(
                "t",
                std::path::Path::new("/x/T.extractor"),
                &format!("{{{{{name}}}}}")
            )
            .error()
            .is_some()
        );
    }
}

#[test]
fn places_count_characters() {
    assert_eq!(place("ab\nçd", 5), (2, 2));
    assert_eq!(place("", 0), (1, 1));
}

#[test]
fn completion_offers_fields_sections_and_filters() {
    let labels = |text: &str| -> Vec<String> {
        complete(text, text.len())
            .map(|(_, s)| s.into_iter().map(|s| s.label).collect())
            .unwrap_or_default()
    };
    assert!(labels("{{").contains(&"value".to_owned()));
    assert_eq!(labels("{{col"), ["column.name", "column.type", "column.kind", "column.index"]);
    assert_eq!(labels("{{#r"), ["rows"]);
    assert!(!labels("{{^").contains(&"rows".to_owned()), "a loop isn't inverted");
    assert_eq!(labels("{{#rows}}{{#columns}}{{/").first().map(String::as_str), Some("columns"));
    assert_eq!(labels("{{value|j"), ["json"]);
    assert_eq!(labels("{{value|sql|pa"), ["pad:"]);
    assert!(labels("text").is_empty());
    assert!(labels("{{value}} x").is_empty());
    assert!(labels("{{! a comment").is_empty());
    let (range, _) = complete("{{column.na", 11).unwrap();
    assert_eq!(range, 2..11);
}

#[test]
fn php_and_python_literals() {
    let one = vec![vec![Cell::Int(1), Cell::Text("it's a \\ $x\nline"), Cell::Null]];
    let t = |f: &str| {
        run(
            &format!(
                "{{{{#rows}}}}{{{{#columns}}}}{{{{value|{f}}}}}{{{{^last}}}},{{{{/last}}}}{{{{/columns}}}}{{{{/rows}}}}"
            ),
            &one,
        )
    };
    assert_eq!(t("php"), "1,'it\\'s a \\\\ $x\nline',null");
    assert_eq!(t("py"), "1,'it\\'s a \\\\ $x\\nline',None");
    let bools = vec![vec![Cell::Bool(true), Cell::Bool(false), Cell::Float(f64::NAN)]];
    let b = |f: &str| {
        run(
            &format!(
                "{{{{#rows}}}}{{{{#columns}}}}{{{{value|{f}}}}} {{{{/columns}}}}{{{{/rows}}}}"
            ),
            &bools,
        )
    };
    assert_eq!(b("php"), "true false null ");
    assert_eq!(b("py"), "True False None ");
    assert_eq!(run("{{table|php}} {{table|py}}", &[]), "'items' 'items'");
}
