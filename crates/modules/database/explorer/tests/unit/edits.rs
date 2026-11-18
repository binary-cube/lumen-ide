use database::{CellChange, TableRef};

use super::*;

#[test]
fn a_file_s_changes_read_as_lines() {
    let mut set = ChangeSet::new(TableRef::new("people"));
    set.changes.push(Change::Delete { key: RowKey::Line { line: 4, fingerprint: 1 } });
    set.changes.push(Change::Update {
        key: RowKey::Line { line: 0, fingerprint: 2 },
        cells: vec![CellChange {
            column: "name".into(),
            old: Value::Text("a".into()),
            new: Value::Null,
        }],
    });
    set.changes.push(Change::Insert { values: vec![("id".into(), Value::Int(7))] });
    let lines = summary(&set);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains('5'), "{lines:?}");
    assert!(lines[1].contains("name: 'a' → NULL"), "{lines:?}");
    assert!(lines[2].contains("id = 7"), "{lines:?}");
}
