use super::*;

#[test]
fn declared_types_fall_in_their_class() {
    use TypeClass::*;
    for (declared, class) in [
        ("INTEGER", Integer),
        ("bigint", Integer),
        ("int4", Integer),
        ("serial", Integer),
        ("varchar(255)", Text),
        ("character varying", Text),
        ("TEXT", Text),
        ("uuid", Text),
        ("boolean", Bool),
        ("tinyint(1)", Bool),
        ("numeric(10, 2)", Decimal),
        ("double precision", Decimal),
        ("REAL", Decimal),
        ("money", Decimal),
        ("timestamp with time zone", DateTime),
        ("DATE", DateTime),
        ("interval", DateTime),
        ("BLOB", Binary),
        ("bytea", Binary),
        ("varbinary(16)", Binary),
        ("jsonb", Json),
        ("int4[]", Other),
        ("_int4", Other),
        ("int4range", Other),
        ("point", Other),
        ("", Other),
        ("geometry", Other),
    ] {
        assert_eq!(TypeClass::of(declared), class, "{declared}");
    }
}

#[test]
fn rows_are_found_by_the_primary_key_then_the_row_id() {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let mut structure = Structure {
        table: TableRef::new("users"),
        columns: vec![id, ColumnInfo::new("name", "TEXT")],
        rowid: true,
        ..Structure::default()
    };
    assert_eq!(structure.key_kind(), KeyKind::Primary(vec!["id".into()]));
    assert_eq!(structure.column("name").map(|c| c.class), Some(TypeClass::Text));
    structure.columns[0].in_key = false;
    assert_eq!(structure.key_kind(), KeyKind::RowId);
    structure.rowid = false;
    assert_eq!(structure.key_kind(), KeyKind::None, "no key: only read");
    assert_eq!(TableRef::in_schema("public", "users").to_string(), "public.users");
}

#[test]
fn a_primary_key_keeps_its_own_order() {
    let mut a = ColumnInfo::new("a", "TEXT");
    let mut b = ColumnInfo::new("b", "TEXT");
    (a.in_key, a.key_position) = (true, Some(2));
    (b.in_key, b.key_position) = (true, Some(1));
    let structure = Structure {
        table: TableRef::new("pairs"),
        columns: vec![a, b, ColumnInfo::new("c", "TEXT")],
        ..Structure::default()
    };
    // `PRIMARY KEY (b, a)`: the key's order, not the columns'.
    assert_eq!(structure.primary_key(), vec!["b".to_owned(), "a".to_owned()]);
}

#[test]
fn the_row_id_is_read_by_a_name_no_column_hides() {
    let mut structure = Structure {
        table: TableRef::new("log"),
        columns: vec![ColumnInfo::new("RowId", "TEXT")],
        rowid: true,
        ..Structure::default()
    };
    assert_eq!(structure.row_id_name(), Some("_rowid_"));
    assert_eq!(structure.key_kind(), KeyKind::RowId);
    structure.columns.push(ColumnInfo::new("_rowid_", "TEXT"));
    assert_eq!(structure.row_id_name(), Some("oid"));
    structure.columns.push(ColumnInfo::new("oid", "TEXT"));
    assert_eq!(structure.row_id_name(), None);
    assert_eq!(structure.key_kind(), KeyKind::None, "all three hidden, no key: only read");
    structure.columns.clear();
    structure.rowid = false;
    assert_eq!(structure.row_id_name(), None);
}

#[test]
fn a_files_rows_are_found_by_their_line_after_any_other_key() {
    let mut structure = Structure {
        table: TableRef::new("people"),
        columns: vec![ColumnInfo::new("name", "TEXT")],
        lines: true,
        ..Structure::default()
    };
    assert_eq!(structure.key_kind(), KeyKind::Line);
    structure.columns[0].in_key = true;
    assert_eq!(structure.key_kind(), KeyKind::Primary(vec!["name".into()]));
}
