use database::{
    CellChange, Change, ChangeSet, ColumnDef, DbError, DefaultValue, Page, RowKey, SchemaChange,
    Session, Value,
};

use crate::CsvSession;
use crate::tests::{TempDir, open, read, table, target, texts};

fn update(key: &RowKey, column: &str, old: Value, new: Value) -> Change {
    Change::Update { key: key.clone(), cells: vec![CellChange { column: column.into(), old, new }] }
}

fn text(t: &str) -> Value {
    Value::Text(t.to_owned())
}

fn set(session: &CsvSession, changes: Vec<Change>) -> ChangeSet {
    ChangeSet { table: table(session), changes }
}

#[test]
fn updates_inserts_and_deletes_write_exactly_the_changed_bytes() {
    let dir = TempDir::new("exact");
    // A byte order mark, `;`, CRLF, a blank line and a field quoted needlessly: all kept.
    let path = dir.file(
        "people.csv",
        b"\xEF\xBB\xBFid;name;note\r\n1;ana;\"plain\"\r\n\r\n2;bob;x\r\n3;cid;\"a;b\"\r\n",
    );
    let mut session = open(&path);
    let rows = read(&mut session, &Page::first(10));
    assert_eq!(rows.rows.len(), 3);
    let changes = vec![
        Change::Insert {
            values: vec![("id".into(), Value::Int(4)), ("name".into(), text("dan; \"d\""))],
        },
        update(&rows.keys[0], "name", text("ana"), text("Ana Maria")),
        Change::Delete { key: rows.keys[1].clone() },
        update(&rows.keys[2], "note", Value::Other("a;b".into()), text("two\nlines")),
    ];
    let applied = session.apply(&set(&session, changes)).unwrap();
    assert_eq!(applied.affected, 4);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"\xEF\xBB\xBFid;name;note\r\n1;Ana Maria;\"plain\"\r\n\r\n3;cid;\"two\nlines\"\r\n\
          4;\"dan; \"\"d\"\"\";\r\n"
    );
    assert_eq!(dir.names(), ["people.csv"], "no copy left");
    // The keys given back are those the rows are read with.
    let rows = read(&mut session, &Page::first(10));
    assert_eq!(applied.inserted, vec![rows.keys[2].clone()]);
    assert_eq!(texts(&rows)[2], ["4", "dan; \"d\"", ""]);
}

#[test]
fn a_file_without_a_last_line_ending_keeps_none_and_quoted_files_stay_quoted() {
    let dir = TempDir::new("ending");
    let path = dir.file("q.csv", b"\"id\",\"name\"\n\"1\",\"ana\"\n\"2\",\"bob\"");
    let mut session = open(&path);
    assert!(session.layout().style.quote_all);
    let rows = read(&mut session, &Page::first(10));
    let changes = vec![Change::Insert {
        values: vec![("id".into(), Value::Int(3)), ("name".into(), text("cid"))],
    }];
    session.apply(&set(&session, changes)).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"\"id\",\"name\"\n\"1\",\"ana\"\n\"2\",\"bob\"\n\"3\",\"cid\""
    );
    // The last row deleted: still no line ending at the end.
    let rows_now = read(&mut session, &Page::first(10));
    assert_eq!(rows_now.rows.len(), 3);
    let changes = vec![Change::Delete { key: rows_now.keys[2].clone() }];
    session.apply(&set(&session, changes)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"\"id\",\"name\"\n\"1\",\"ana\"\n\"2\",\"bob\"");
    drop(rows);

    // A row updated to nothing but one empty field is still a row, not a blank line.
    let one = dir.file("one.csv", b"name\nana\nbob\n");
    let mut session = open(&one);
    let rows = read(&mut session, &Page::first(10));
    session
        .apply(&set(&session, vec![update(&rows.keys[0], "name", text("ana"), Value::Null)]))
        .unwrap();
    assert_eq!(std::fs::read(&one).unwrap(), b"name\n\"\"\nbob\n");
    assert_eq!(read(&mut session, &Page::first(10)).rows.len(), 2);
}

#[test]
fn a_file_changed_since_it_was_read_is_a_conflict_and_left_as_it_is() {
    let dir = TempDir::new("changed");
    let path = dir.file("people.csv", b"id,name\n1,ana\n2,bob\n");
    let mut session = open(&path);
    let rows = read(&mut session, &Page::first(10));
    // Another program adds a row.
    let theirs = b"id,name\n1,ana\n2,bob\n3,cid\n".to_vec();
    std::fs::write(&path, &theirs).unwrap();
    let changes = vec![update(&rows.keys[0], "name", text("ana"), text("ANA"))];
    assert!(matches!(session.apply(&set(&session, changes)), Err(DbError::Conflict(_))));
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(dir.names(), ["people.csv"]);
    // Read again, it can be changed.
    let rows = read(&mut session, &Page::first(10));
    assert_eq!(rows.rows.len(), 3);
    let changes = vec![update(&rows.keys[0], "name", text("ana"), text("ANA"))];
    session.apply(&set(&session, changes)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"id,name\n1,ANA\n2,bob\n3,cid\n");
}

#[test]
fn a_change_of_the_same_size_and_time_is_found_by_the_hash() {
    let dir = TempDir::new("same-size");
    let path = dir.file("people.csv", b"id,name\n1,ana\n2,bob\n");
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut session = open(&path);
    let rows = read(&mut session, &Page::first(10));
    // `bob` becomes `BOB` in place, the time put back: only the bytes tell.
    let theirs = b"id,name\n1,ana\n2,BOB\n".to_vec();
    std::fs::write(&path, &theirs).unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    let changes = vec![update(&rows.keys[0], "name", text("ana"), text("ANA"))];
    assert!(matches!(session.apply(&set(&session, changes)), Err(DbError::Conflict(_))));
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(dir.names(), ["people.csv"]);
}

#[test]
fn a_row_not_as_it_was_read_writes_nothing_and_leaves_no_copy() {
    let dir = TempDir::new("row");
    let bytes = b"id,name\n1,ana\n2,bob\n3,cid\n";
    let path = dir.file("people.csv", bytes);
    let mut session = open(&path);
    let rows = read(&mut session, &Page::first(10));
    let wrong = RowKey::Line { line: 1, fingerprint: 42 };
    let cases = vec![
        // Its fingerprint isn't the row's (the copy was under way: removed).
        vec![Change::Delete { key: rows.keys[0].clone() }, Change::Delete { key: wrong }],
        // Past the last row.
        vec![Change::Delete { key: RowKey::Line { line: 9, fingerprint: 0 } }],
        // Deleted twice; changed and deleted.
        vec![
            Change::Delete { key: rows.keys[0].clone() },
            Change::Delete { key: rows.keys[0].clone() },
        ],
        vec![
            update(&rows.keys[1], "name", text("bob"), text("rob")),
            Change::Delete { key: rows.keys[1].clone() },
        ],
    ];
    for changes in cases {
        let result = session.apply(&set(&session, changes.clone()));
        assert!(matches!(result, Err(DbError::Conflict(_))), "{changes:?}: {result:?}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(dir.names(), ["people.csv"]);
    }
    let bytes_in = vec![Change::Insert { values: vec![("name".into(), Value::Bytes(vec![1]))] }];
    assert!(matches!(session.apply(&set(&session, bytes_in)), Err(DbError::Query(_))));
    let unknown = vec![Change::Insert { values: vec![("nope".into(), text("x"))] }];
    assert!(matches!(session.apply(&set(&session, unknown)), Err(DbError::Query(_))));
    let by_id = vec![Change::Delete { key: RowKey::RowId(1) }];
    assert!(matches!(session.apply(&set(&session, by_id)), Err(DbError::Query(_))));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_folder_that_cant_be_written_fails_and_leaves_the_file() {
    let dir = TempDir::new("folder");
    let bytes = b"id,name\n1,ana\n";
    let path = dir.file("people.csv", bytes);
    let mut session = open(&path);
    let rows = read(&mut session, &Page::first(10));
    // The file gone from under it: nothing made, nothing left.
    std::fs::remove_file(&path).unwrap();
    let changes = vec![Change::Delete { key: rows.keys[0].clone() }];
    assert!(session.apply(&set(&session, changes)).is_err());
    assert!(dir.names().is_empty());
}

#[test]
fn columns_are_added_renamed_and_dropped_in_one_copy() {
    let dir = TempDir::new("columns");
    let path = dir.file("people.csv", b"id,name,age\r\n1,\"ana\",31\r\n2,bob\r\n");
    let mut session = open(&path);
    let t = table(&session);
    let changes = vec![
        SchemaChange::AddColumn {
            table: t.clone(),
            column: ColumnDef::new("team", "TEXT").default(DefaultValue::Value(text("red, blue"))),
        },
        SchemaChange::RenameColumn {
            table: t.clone(),
            from: "name".into(),
            to: "full name".into(),
        },
        SchemaChange::DropColumn { table: t.clone(), column: "age".into() },
    ];
    assert_eq!(session.alter(&changes).map(|a| a.affected), Ok(3));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"id,full name,team\r\n1,\"ana\",\"red, blue\"\r\n2,bob,\"red, blue\"\r\n"
    );
    let names: Vec<&str> = session.layout().columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "full name", "team"]);
    assert_eq!(texts(&read(&mut session, &Page::first(1))), [["1", "ana", "red, blue"]]);

    // Only what a file can do.
    for refused in [
        SchemaChange::DropTable { table: t.clone() },
        SchemaChange::AlterColumn {
            table: t.clone(),
            column: "id".into(),
            to: ColumnDef::new("id", "TEXT"),
        },
    ] {
        assert!(matches!(session.alter(&[refused]), Err(DbError::Unsupported(_))));
    }
    let renamed_only = SchemaChange::AlterColumn {
        table: t.clone(),
        column: "id".into(),
        to: ColumnDef::new("key", ""),
    };
    session.alter(&[renamed_only]).unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"key,full name,team\r\n"));
    let all: Vec<SchemaChange> = ["key", "full name", "team"]
        .iter()
        .map(|c| SchemaChange::DropColumn { table: t.clone(), column: (*c).into() })
        .collect();
    assert!(matches!(session.alter(&all), Err(DbError::Query(_))));

    // Without a header there's no name to change.
    let plain = dir.file("plain.csv", b"1,2\n3,4\n");
    let mut session = open(&plain);
    let rename = SchemaChange::RenameColumn {
        table: table(&session),
        from: "column_1".into(),
        to: "a".into(),
    };
    assert!(matches!(session.alter(&[rename]), Err(DbError::Unsupported(_))));
    let add = SchemaChange::AddColumn { table: table(&session), column: ColumnDef::new("c", "") };
    session.alter(&[add]).unwrap();
    assert_eq!(std::fs::read(&plain).unwrap(), b"1,2,\n3,4,\n");
}

#[test]
fn a_column_moves_with_its_fields_as_written() {
    let dir = TempDir::new("move");
    let path = dir.file("people.csv", b"id,name,age\r\n1,\"ana\",31\r\n2,bob\r\n");
    let mut session = open(&path);
    let t = table(&session);
    let moved = SchemaChange::MoveColumn { table: t.clone(), column: "age".into(), to: 0 };
    session.alter(&[moved]).unwrap();
    // A record shorter than the header gets the moved field empty; quotes kept.
    assert_eq!(std::fs::read(&path).unwrap(), b"age,id,name\r\n31,1,\"ana\"\r\n,2,bob\r\n");
    let names: Vec<&str> = session.layout().columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["age", "id", "name"]);
    let past = SchemaChange::MoveColumn { table: t, column: "age".into(), to: 9 };
    session.alter(&[past]).unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"id,name,age\r\n"));
}

#[test]
fn a_tsv_file_is_written_with_tabs() {
    let dir = TempDir::new("tsv");
    let path = dir.file("data.tsv", b"a\tb\n1\tx y\n");
    let mut session = CsvSession::open(&target(&path)).unwrap();
    let rows = read(&mut session, &Page::first(10));
    let changes = vec![update(&rows.keys[0], "b", text("x y"), text("tab\there"))];
    session.apply(&set(&session, changes)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a\tb\n1\t\"tab\there\"\n");
}

#[test]
fn nothing_holds_the_file_on_unix() {
    // Only Windows retries a file another program holds.
    let error = std::io::Error::from_raw_os_error(32);
    assert_eq!(super::held(&error), cfg!(windows));
}
