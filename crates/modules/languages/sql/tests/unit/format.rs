use std::path::PathBuf;

use super::*;

fn request(path: &str, text: &str, scope: FormatScope) -> FormatRequest {
    FormatRequest {
        project: None,
        path: PathBuf::from(path),
        language: "sql".into(),
        text: text.into(),
        scope,
    }
}

fn applied(text: &str, formatted: &Formatted) -> String {
    let mut out = text.to_owned();
    if let Formatted::Changed { edit, .. } = formatted {
        out.replace_range(edit.range.clone(), &edit.text);
    }
    out
}

#[test]
fn a_file_is_laid_out_whole_its_change_narrowed() {
    let text = "SELECT a from t";
    let formatted =
        lay_out(&request("/p/q.sql", text, FormatScope::Whole), &FormatOptions::default());
    let Formatted::Changed { edit, left } = &formatted else { panic!("{formatted:?}") };
    assert_eq!((edit.range.clone(), edit.text.as_str(), *left), (8..13, "\nFROM", 0));
    assert_eq!(applied(text, &formatted), "SELECT a\nFROM t");
    let again = lay_out(
        &request("/p/q.sql", "SELECT a\nFROM t", FormatScope::Whole),
        &FormatOptions::default(),
    );
    assert_eq!(again, Formatted::Unchanged { left: 0 });
}

#[test]
fn the_statement_at_the_cursor_or_the_selection_s_are_laid_out() {
    let text = "select 1;\nselect a from t;\nselect 3;";
    let at = text.find("from").unwrap();
    let formatted =
        lay_out(&request("/p/q.sql", text, FormatScope::At(at)), &FormatOptions::default());
    assert_eq!(applied(text, &formatted), "select 1;\nSELECT a\nFROM t;\nselect 3;");
    let all = 0..text.len();
    let formatted =
        lay_out(&request("/p/q.sql", text, FormatScope::Range(all)), &FormatOptions::default());
    assert_eq!(applied(text, &formatted), "SELECT 1;\nSELECT a\nFROM t;\nSELECT 3;");
    let blank = "select 1;\n\n\n\nselect 2;";
    let nothing =
        lay_out(&request("/p/q.sql", blank, FormatScope::At(12)), &FormatOptions::default());
    assert_eq!(nothing, Formatted::Nothing);
}

#[test]
fn a_file_s_name_tells_its_dialect() {
    // `$$ … $$` is a body in PostgreSQL only: elsewhere its insides are tokens.
    let text = "select $$ a  ,  b $$";
    let pg = lay_out(&request("/p/f.pgsql", text, FormatScope::Whole), &FormatOptions::default());
    assert_eq!(applied(text, &pg), "SELECT $$ a  ,  b $$");
    assert_eq!(flavor(None, Path::new("x.sqlite.sql")), Flavor::Sqlite);
    assert_eq!(flavor(None, Path::new("x.sql")), Flavor::Generic);
    // A project's document whose module knows nothing of it: by its name too.
    assert_eq!(
        flavor(Some(ProjectId::new()), Path::new("x.psql")),
        Flavor::Postgres { version: None }
    );
}

#[test]
fn unfinished_statements_are_counted_as_left() {
    let text = "select  1;\nselect 'open";
    let formatted =
        lay_out(&request("/p/q.sql", text, FormatScope::Whole), &FormatOptions::default());
    let Formatted::Changed { left, .. } = formatted else { panic!("{formatted:?}") };
    assert_eq!(left, 1);
}
