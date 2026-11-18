use super::*;
use crate::cell::Kind;
use crate::extractor::{Input, Options, extract};
use crate::sql::AnsiSql;

use crate::test_support::{cols, rows};

/// MySQL-like quoting: what the dialect gives is what's written.
#[derive(Debug)]
struct Backticks;

impl SqlStyle for Backticks {
    fn name(&self, name: &str) -> String {
        format!("`{name}`")
    }
    fn literal(&self, value: Cell<'_>) -> String {
        match value {
            Cell::Bool(b) => i64::from(b).to_string(),
            Cell::Text(t) => format!("'{}'", t.replace('\\', "\\\\").replace('\'', "\\'")),
            other => crate::sql::ansi_literal(other),
        }
    }
}

fn run(e: &dyn Extractor, columns: Vec<Column>, rows: &[Vec<Cell<'_>>]) -> String {
    extract(e, Input::new("t", columns), Options::default(), rows)
}

#[test]
fn inserts_quote_names_and_texts() {
    let e = SqlInserts { multirow: false };
    assert_eq!(
        extract(&e, Input::new("my \"t\"", cols()), Options::default(), &rows()),
        "INSERT INTO \"my \"\"t\"\"\" (\"id\", \"name\", \"price\") VALUES (1, 'Ann', 2.5);\n\
         INSERT INTO \"my \"\"t\"\"\" (\"id\", \"name\", \"price\") VALUES (2, 'O''Hara, \"Jo\"\nline', NULL);"
    );
    let one = vec![Column::new("b", Kind::Binary)];
    assert_eq!(
        run(
            &e,
            one,
            &[vec![Cell::Bytes(&[1, 255])], vec![Cell::Bool(false)], vec![Cell::Float(f64::NAN)]]
        ),
        "INSERT INTO \"t\" (\"b\") VALUES (X'01ff');\nINSERT INTO \"t\" (\"b\") VALUES (FALSE);\nINSERT INTO \"t\" (\"b\") VALUES (NULL);"
    );
}

#[test]
fn the_dialect_writes_names_and_values() {
    let input = Input::new("t", cols()).with_sql(std::sync::Arc::new(Backticks));
    let rows = vec![vec![Cell::Int(1), Cell::Text("it's \\"), Cell::Bool(true)]];
    assert_eq!(
        extract(&SqlInserts { multirow: false }, input, Options::default(), &rows),
        "INSERT INTO `t` (`id`, `name`, `price`) VALUES (1, 'it\\'s \\\\', 1);"
    );
}

#[test]
fn multirow_inserts_many_rows_a_statement() {
    let e = SqlInserts { multirow: true };
    assert_eq!(
        run(&e, cols(), &rows()),
        "INSERT INTO \"t\" (\"id\", \"name\", \"price\")\nVALUES (1, 'Ann', 2.5),\n       (2, 'O''Hara, \"Jo\"\nline', NULL);"
    );
    assert_eq!(run(&e, cols(), &[]), "");
    // A block at a time gives the same text, and a new statement after MULTIROW_ROWS.
    let many: Vec<Vec<Cell<'_>>> =
        (0..MULTIROW_ROWS as i64 + 2).map(|i| vec![Cell::Int(i), Cell::Null, Cell::Null]).collect();
    let whole = run(&e, cols(), &many);
    let mut x = crate::Extraction::new(&e, Input::new("t", cols()), Options::default());
    let mut pieces = x.begin();
    for block in many.chunks(7) {
        pieces.push_str(&x.rows(block));
    }
    pieces.push_str(&x.finish());
    assert_eq!(pieces, whole);
    assert_eq!(whole.matches("INSERT INTO").count(), 2);
    assert!(whole.contains("(999, NULL, NULL);\nINSERT INTO"));
    assert!(whole.ends_with("(1001, NULL, NULL);"));
}

#[test]
fn computed_and_generated_columns_are_skipped_when_asked() {
    let mut columns = cols();
    columns[0].generated = true;
    columns[2].computed = true;
    let row = vec![vec![Cell::Int(1), Cell::Text("a"), Cell::Int(3)]];
    let e = SqlInserts { multirow: false };
    let with = |o: Options| extract(&e, Input::new("t", columns.clone()), o, &row);
    assert_eq!(
        with(Options::default()),
        "INSERT INTO \"t\" (\"id\", \"name\", \"price\") VALUES (1, 'a', 3);"
    );
    assert_eq!(
        with(Options { skip_generated: true, skip_computed: false }),
        "INSERT INTO \"t\" (\"name\", \"price\") VALUES ('a', 3);"
    );
    assert_eq!(
        with(Options { skip_generated: true, skip_computed: true }),
        "INSERT INTO \"t\" (\"name\") VALUES ('a');"
    );
    // Not for the formats that don't write rows into a table.
    let csv = crate::Delimited(crate::CsvFormat::new("c", "C", ",", false));
    assert_eq!(
        extract(
            &csv,
            Input::new("t", columns.clone()),
            Options { skip_generated: true, skip_computed: true },
            &row
        ),
        "1,a,3"
    );
    // Every column skipped: none is.
    let all: Vec<Column> = columns
        .iter()
        .cloned()
        .map(|mut c| {
            c.computed = true;
            c
        })
        .collect();
    assert_eq!(
        extract(
            &e,
            Input::new("t", all),
            Options { skip_generated: false, skip_computed: true },
            &row
        ),
        "INSERT INTO \"t\" (\"id\", \"name\", \"price\") VALUES (1, 'a', 3);"
    );
}

#[test]
fn updates_find_rows_by_their_key() {
    assert_eq!(
        run(&SqlUpdates, cols(), &rows()),
        "UPDATE \"t\" SET \"name\" = 'Ann', \"price\" = 2.5 WHERE \"id\" = 1;\n\
         UPDATE \"t\" SET \"name\" = 'O''Hara, \"Jo\"\nline', \"price\" = NULL WHERE \"id\" = 2;"
    );
    // No key: every column finds it (a NULL by IS NULL), every column set.
    let plain = vec![Column::new("a", Kind::Text), Column::new("b", Kind::Integer)];
    assert_eq!(
        run(&SqlUpdates, plain, &[vec![Cell::Text("x"), Cell::Null]]),
        "UPDATE \"t\" SET \"a\" = 'x', \"b\" = NULL WHERE \"a\" = 'x' AND \"b\" IS NULL;"
    );
    // Only the key selected: it's set too.
    let key = vec![Column::new("id", Kind::Integer).keyed()];
    assert_eq!(
        run(&SqlUpdates, key, &[vec![Cell::Int(4)]]),
        "UPDATE \"t\" SET \"id\" = 4 WHERE \"id\" = 4;"
    );
}

#[test]
fn a_where_clause_matches_the_rows() {
    assert_eq!(run(&WhereClause, cols(), &rows()), "\"id\" IN (1, 2)");
    let one = vec![Column::new("name", Kind::Text)];
    assert_eq!(
        run(
            &WhereClause,
            one.clone(),
            &[vec![Cell::Text("a")], vec![Cell::Null], vec![Cell::Text("b'")]]
        ),
        "\"name\" IN ('a', 'b''') OR \"name\" IS NULL"
    );
    assert_eq!(run(&WhereClause, one.clone(), &[vec![Cell::Null]]), "\"name\" IS NULL");
    assert_eq!(run(&WhereClause, one, &[]), "");
    let two = vec![Column::new("a", Kind::Integer), Column::new("b", Kind::Text)];
    assert_eq!(
        run(
            &WhereClause,
            two,
            &[vec![Cell::Int(1), Cell::Text("x")], vec![Cell::Int(2), Cell::Null]]
        ),
        "(\"a\" = 1 AND \"b\" = 'x')\nOR (\"a\" = 2 AND \"b\" IS NULL)"
    );
    let _ = AnsiSql;
}
