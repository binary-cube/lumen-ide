use super::*;

fn cols() -> Vec<GridColumn> {
    let mut id = GridColumn::new("id", "INTEGER", CellClass::Integer);
    id.key = true;
    id.generated = true;
    let mut total = GridColumn::new("total", "REAL", CellClass::Decimal);
    total.computed = true;
    vec![id, GridColumn::new("name", "TEXT", CellClass::Text), total]
}

fn rows() -> Vec<Vec<CellRef<'static>>> {
    vec![
        vec![CellRef::Int(1), CellRef::Text("Ann"), CellRef::Float(2.5)],
        vec![CellRef::Int(2), CellRef::Text("O'Hara, \"Jo\"\nline"), CellRef::Null],
    ]
}

fn by(id: &str) -> std::sync::Arc<dyn Extractor> {
    extractors::Catalog::new().find(None, id).unwrap()
}

#[test]
fn the_grid_s_columns_keep_their_marks() {
    let c = column_of(&cols()[0]);
    assert!(c.key && c.generated && !c.computed);
    assert_eq!(c.kind, extractors::Kind::Integer);
    assert_eq!(c.type_name, "INTEGER");
    assert!(column_of(&cols()[2]).computed);
    assert_eq!(cell_of(CellRef::Bytes(&[1])), extractors::Cell::Bytes(&[1]));
}

#[test]
fn rows_are_written_by_the_extractor_asked() {
    let o = Options::default();
    assert_eq!(
        copy_text(by("tsv").as_ref(), "t", &cols(), None, o, &rows()),
        "1\tAnn\t2.5\n2\t\"O'Hara, \"\"Jo\"\"\nline\"\t"
    );
    assert_eq!(
        copy_text(by("csv").as_ref(), "t", &cols(), None, o, &rows()[..1]),
        "id,name,total\n1,Ann,2.5"
    );
    assert_eq!(
        copy_text(by("sql").as_ref(), "t", &cols(), None, o, &rows()[..1]),
        "INSERT INTO \"t\" (\"id\", \"name\", \"total\") VALUES (1, 'Ann', 2.5);"
    );
    // Bytes are written whole, not as their size.
    assert_eq!(
        copy_text(
            by("tsv").as_ref(),
            "t",
            &cols()[..1],
            None,
            o,
            &[vec![CellRef::Bytes(&[0xca, 0xfe])]]
        ),
        "0xcafe"
    );
}

#[test]
fn the_skips_and_the_source_s_sql_are_used() {
    let skip = Options { skip_computed: true, skip_generated: true };
    assert_eq!(
        copy_text(by("sql").as_ref(), "t", &cols(), None, skip, &rows()[..1]),
        "INSERT INTO \"t\" (\"name\") VALUES ('Ann');"
    );
    #[derive(Debug)]
    struct Brackets;
    impl extractors::SqlStyle for Brackets {
        fn name(&self, name: &str) -> String {
            format!("[{name}]")
        }
        fn literal(&self, value: extractors::Cell<'_>) -> String {
            extractors::ansi_literal(value)
        }
    }
    let sql: std::sync::Arc<dyn extractors::SqlStyle> = std::sync::Arc::new(Brackets);
    assert_eq!(
        copy_text(
            by("sql-update").as_ref(),
            "t",
            &cols(),
            Some(sql),
            Options::default(),
            &rows()[..1]
        ),
        "UPDATE [t] SET [name] = 'Ann', [total] = 2.5 WHERE [id] = 1;"
    );
}

#[test]
fn a_stream_in_blocks_writes_the_same() {
    let json = by("json");
    let whole = copy_text(json.as_ref(), "t", &cols(), None, Options::default(), &rows());
    let mut s = CopyStream::new(json.as_ref(), "t", &cols(), None, Options::default());
    let mut pieces = s.begin();
    pieces.push_str(&s.rows(&rows()[..1]));
    pieces.push_str(&s.rows(&rows()[1..]));
    pieces.push_str(&s.finish());
    assert_eq!(pieces, whole);
    assert_eq!(s.written(), 2);
}
