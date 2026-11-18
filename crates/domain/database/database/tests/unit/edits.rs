use super::*;
use crate::Generated;

fn columns() -> Vec<ColumnInfo> {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let mut total = ColumnInfo::new("total", "INTEGER");
    total.generated = Some(Generated { expression: "1".into(), stored: false });
    vec![id, ColumnInfo::new("name", "TEXT"), total]
}

fn key(id: i64) -> RowKey {
    RowKey::Primary(vec![("id".into(), Value::Int(id))])
}

fn row(id: i64, name: &str) -> Vec<Value> {
    vec![Value::Int(id), Value::Text(name.into()), Value::Int(0)]
}

#[test]
fn a_cell_changed_back_is_no_change() {
    let mut e = Edits::default();
    e.set(&key(1), &row(1, "a"), 1, Value::Text("b".into()));
    assert_eq!(e.count(), 1);
    assert_eq!(e.value(&key(1), 1), Some(&Value::Text("b".into())));
    e.set(&key(1), &row(1, "a"), 1, Value::Text("a".into()));
    assert!(e.is_empty());
    e.set(&key(2), &row(2, "x"), 1, Value::Text("x".into()));
    assert!(e.is_empty(), "the value as read: nothing");
}

#[test]
fn changes_become_one_change_set_deletes_updates_inserts() {
    let mut e = Edits::default();
    e.set(&key(1), &row(1, "a"), 1, Value::Text("b".into()));
    e.set(&key(2), &row(2, "c"), 1, Value::Null);
    e.toggle_delete(&key(2));
    let id = e.insert(1000, 3, None);
    e.set_new(id, 1, Value::Text("new".into()));
    e.set_new(id, 2, Value::Int(9));
    assert_eq!(e.count(), 3, "a deleted row's cells don't count");
    let set = e.change_set(TableRef::new("t"), &columns());
    assert_eq!(
        set.changes,
        vec![
            Change::Delete { key: key(2) },
            Change::Update {
                key: key(1),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("a".into()),
                    new: Value::Text("b".into()),
                }],
            },
            // The generated column is never written; the key is the engine's.
            Change::Insert { values: vec![("name".into(), Value::Text("new".into()))] },
        ]
    );
}

#[test]
fn rows_are_unmarked_dropped_and_reverted() {
    let mut e = Edits::default();
    e.toggle_delete(&key(1));
    e.toggle_delete(&key(1));
    assert!(e.is_empty());
    let a = e.insert(1000, 3, None);
    let b = e.insert(1000, 3, Some(vec![None, Some(Value::Text("copy".into()))]));
    assert_ne!(a, b);
    assert_eq!(e.new_row(b).unwrap().values.len(), 3);
    e.drop_new(a);
    assert_eq!(e.inserted().len(), 1);
    e.set(&key(5), &row(5, "a"), 1, Value::Text("z".into()));
    e.toggle_delete(&key(5));
    e.revert_row(&key(5));
    assert_eq!(e.count(), 1);
    e.clear();
    assert!(e.is_empty());
}

// ─── A query's result ───

fn result(names: &[&str]) -> Vec<Column> {
    names.iter().map(|n| Column { name: (*n).into(), type_name: String::new() }).collect()
}

fn shape_of(sql: &str) -> QueryShape {
    let flavor = crate::sql::Flavor::Sqlite;
    crate::sql::query_shape(sql, &crate::sql::lex(sql, flavor), flavor).unwrap()
}

/// `people (id INTEGER PRIMARY KEY, name TEXT, email TEXT NOT NULL UNIQUE, total
/// GENERATED)`, with a row id.
fn people() -> Structure {
    let mut columns = columns();
    let mut email = ColumnInfo::new("email", "TEXT");
    email.nullable = false;
    columns.push(email);
    Structure {
        table: TableRef::new("people"),
        columns,
        indexes: vec![crate::IndexInfo::new("people_email", vec!["email".into()], true)],
        rowid: true,
        ..Structure::default()
    }
}

#[test]
fn a_result_s_columns_are_its_table_s_and_its_rows_found_by_its_key() {
    let s = shape_of("SELECT * FROM people");
    let e = editable_result(&s, &result(&["id", "name", "total", "email"]), people()).unwrap();
    assert_eq!(e.columns, [Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(e.key, ResultKey::Columns(vec![("id".into(), 0)]));
    assert!(e.writable(1) && !e.writable(2), "a generated column is never written");
    assert!(e.in_key(0) && !e.in_key(1));
    let row = [Value::Int(7), Value::Text("a".into()), Value::Int(0), Value::Text("@".into())];
    assert_eq!(e.key_of(&row), Some(key(7)));
    assert_eq!(e.key_of(&[Value::Null]), None, "no row is found by a NULL");

    // Renamed, reordered, an expression and a column shown twice.
    let s = shape_of("SELECT name AS n, upper(name), id, name FROM people");
    let e = editable_result(&s, &result(&["n", "upper(name)", "id", "name"]), people()).unwrap();
    assert_eq!(e.columns, [Some(1), None, Some(0), None]);
    assert_eq!(e.key, ResultKey::Columns(vec![("id".into(), 2)]));
    let infos = e.column_infos(&result(&["n", "upper(name)", "id", "name"]));
    assert_eq!(infos[0].name, "name");
    assert!(infos[1].generated.is_some(), "an expression is never written");
    // Its changes are the table's columns'.
    let mut edits = Edits::default();
    let row =
        [Value::Text("a".into()), Value::Text("A".into()), Value::Int(7), Value::Text("a".into())];
    edits.set(&key(7), &row, 0, Value::Text("b".into()));
    let id = edits.insert(
        1000,
        4,
        Some(vec![Some(Value::Text("c".into())), Some(Value::Text("C".into())), None, None]),
    );
    let _ = id;
    let set = edits.change_set(TableRef::new("people"), &infos);
    assert_eq!(
        set.changes,
        vec![
            Change::Update {
                key: key(7),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("a".into()),
                    new: Value::Text("b".into()),
                }],
            },
            Change::Insert { values: vec![("name".into(), Value::Text("c".into()))] },
        ]
    );
}

#[test]
fn without_the_primary_key_a_row_id_or_a_unique_not_null_key_finds_the_rows() {
    let s = shape_of("SELECT rowid, name FROM people");
    let e = editable_result(&s, &result(&["rowid", "name"]), people()).unwrap();
    assert_eq!(e.key, ResultKey::RowId(0));
    assert_eq!(e.key_of(&[Value::Int(3), Value::Null]), Some(RowKey::RowId(3)));
    assert!(!e.writable(0), "the row id isn't written");

    let s = shape_of("SELECT email, name FROM people");
    let e = editable_result(&s, &result(&["email", "name"]), people()).unwrap();
    assert_eq!(e.key, ResultKey::Columns(vec![("email".into(), 0)]));

    // A nullable unique column isn't a key.
    let mut loose = people();
    loose.columns[3].nullable = true;
    let s = shape_of("SELECT email, name FROM people");
    assert_eq!(
        editable_result(&s, &result(&["email", "name"]), loose),
        Err(NotEditable::KeyMissing(vec!["id".into()]))
    );
}

#[test]
fn a_result_without_its_key_or_a_table_without_one_is_read_only() {
    let s = shape_of("SELECT name FROM people");
    assert_eq!(
        editable_result(&s, &result(&["name"]), people()),
        Err(NotEditable::KeyMissing(vec!["id".into()]))
    );
    let heap = Structure {
        table: TableRef::new("v"),
        columns: vec![ColumnInfo::new("a", "TEXT")],
        ..Structure::default()
    };
    let s = shape_of("SELECT * FROM v");
    assert_eq!(editable_result(&s, &result(&["a"]), heap), Err(NotEditable::NoKey));
    // The engine's columns aren't what the query names.
    let s = shape_of("SELECT * FROM people");
    assert_eq!(
        editable_result(&s, &result(&["id", "name"]), people()),
        Err(NotEditable::Unmatched)
    );
    assert_eq!(
        editable_result(&s, &result(&["id", "nom", "total", "email"]), people()),
        Err(NotEditable::Unmatched)
    );
}

/// Thousands of rows deleted and changed: each found by its key (floats and texts in it),
/// unmarked and reverted rows gone from the lookups too, the order kept for what's submitted.
#[test]
fn many_rows_are_found_by_their_keys() {
    let key = |i: i64| {
        RowKey::Primary(vec![
            ("id".into(), Value::Int(i)),
            ("w".into(), Value::Float(i as f64 / 2.0)),
        ])
    };
    let mut e = Edits::default();
    for i in 0..5000 {
        e.toggle_delete(&key(i));
    }
    for i in 4000..6000 {
        e.set(&key(i), &[Value::Int(i)], 0, Value::Int(-i));
    }
    assert!(e.is_deleted(&key(42)) && !e.is_deleted(&key(5000)));
    assert_eq!(e.value(&key(4500), 0), Some(&Value::Int(-4500)));
    e.toggle_delete(&key(42));
    assert!(!e.is_deleted(&key(42)), "unmarked");
    assert!(e.is_deleted(&key(43)), "the others still found after it left");
    e.revert_row(&key(4500));
    assert_eq!(e.value(&key(4500), 0), None);
    assert!(!e.is_deleted(&key(4500)));
    assert_eq!(e.value(&key(4501), 0), Some(&Value::Int(-4501)), "the rest still found");
    // Set back to what was read: no change left for the row.
    e.set(&key(5500), &[Value::Int(5500)], 0, Value::Int(5500));
    assert_eq!(e.value(&key(5500), 0), None);
    assert_eq!(e.value(&key(5501), 0), Some(&Value::Int(-5501)));
    assert_eq!(e.deleted().first(), Some(&key(0)), "in the order marked");
    assert_eq!(e.deleted().len(), 4998);
}
