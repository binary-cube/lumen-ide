//! A result's rows in chunks (shared, counted), big cells cut, the console's memory kept
//! within its limit, the unpinned results replaced by the next run.

use super::*;

fn row(i: i64) -> Vec<Value> {
    vec![Value::Int(i), Value::Text(format!("row {i}"))]
}

#[test]
fn rows_come_in_chunks_read_by_place() {
    let mut buffer = ResultBuffer::default();
    for i in 0..2500 {
        buffer.push(row(i));
    }
    assert_eq!(buffer.len(), 2500);
    let (chunks, starts, sealed) = buffer.chunks();
    assert_eq!((chunks.len(), sealed), (2, 2000), "the open one isn't shared yet");
    assert_eq!(starts, [0, 1000]);
    assert_eq!(buffer.row(2499).unwrap()[0], Value::Int(2499), "read before it's sealed");
    buffer.seal();
    let (chunks, starts, sealed) = buffer.chunks();
    assert_eq!((chunks.len(), sealed), (3, 2500));
    assert_eq!(row_in(&chunks, &starts, 1000).unwrap()[0], Value::Int(1000));
    assert_eq!(row_in(&chunks, &starts, 2499).unwrap()[1], Value::Text("row 2499".into()));
    assert!(row_in(&chunks, &starts, 2500).is_none());
    assert!(buffer.bytes() > 2500 * 7, "the text counted");
    buffer.clear();
    assert!(buffer.is_empty() && buffer.bytes() == 0);
}

#[test]
fn a_big_cell_is_kept_cut() {
    let mut buffer = ResultBuffer::default();
    buffer.push(vec![Value::Text("é".repeat(CELL_LIMIT)), Value::Bytes(vec![7; CELL_LIMIT * 2])]);
    buffer.push(vec![Value::Text("small".into()), Value::Bytes(vec![1, 2])]);
    assert_eq!(buffer.cut(), 2);
    let Value::Text(text) = &buffer.row(0).unwrap()[0] else { panic!() };
    assert!(text.len() < CELL_LIMIT + 64, "{}", text.len());
    assert!(text.contains("KB"), "its size said");
    let Value::Bytes(bytes) = &buffer.row(0).unwrap()[1] else { panic!() };
    assert_eq!(bytes.len(), CELL_LIMIT);
    assert_eq!(buffer.row(1).unwrap()[0], Value::Text("small".into()));
    assert!(buffer.bytes() < 3 * CELL_LIMIT);
    // Where they are (read whole for their editors), until the rows are let go.
    assert_eq!(buffer.cut_at(), [(0, 0), (0, 1)]);
    let epoch = buffer.epoch();
    buffer.clear();
    assert!(buffer.cut_at().is_empty());
    assert_eq!(buffer.epoch(), epoch + 1, "their places aren't the same any more");
}

fn set(results: &mut Results, rows: i64, pinned: bool) -> ServiceId {
    results.next += 1;
    let mut set = ResultSet::new(results.next, "r".into(), "SELECT 1".into(), 0..8);
    set.rerunnable = true;
    for i in 0..rows {
        set.rows.push(row(i));
    }
    set.rows.seal();
    set.pinned = pinned;
    let id = set.id;
    results.sets.push(set);
    id
}

/// Past its limit, the oldest unpinned results let their rows go (then the pinned ones),
/// never the one being read.
#[test]
fn the_oldest_unpinned_results_let_their_rows_go() {
    let mut results = Results::default();
    let pinned = set(&mut results, 100, true);
    let old = set(&mut results, 100, false);
    let newer = set(&mut results, 100, false);
    let one = results.get(old).unwrap().rows.bytes();
    let limit = one * 2 + 10;
    assert_eq!(results.trim(limit, Some(newer)), 1);
    assert!(results.get(old).unwrap().dropped);
    assert!(!results.get(pinned).unwrap().dropped && !results.get(newer).unwrap().dropped);
    assert!(results.bytes() <= limit);
    assert_eq!(results.trim(one, Some(newer)), 1, "then the pinned one");
    assert!(results.get(pinned).unwrap().dropped);
    assert_eq!(results.trim(0, Some(newer)), 0, "never the one being read");
    assert!(!results.get(newer).unwrap().dropped);
}

#[test]
fn a_new_run_replaces_the_unpinned_results() {
    let mut results = Results::default();
    let kept = set(&mut results, 1, true);
    let gone = set(&mut results, 1, false);
    results.get_mut(gone).unwrap().more = More::Cursor(CursorToken("c".into()));
    let cursors = results.replace_unpinned();
    assert_eq!(cursors, [CursorToken("c".into())]);
    assert_eq!(results.sets.len(), 1);
    assert_eq!(results.sets[0].id, kept);
    results.get_mut(kept).unwrap().more = More::Cursor(CursorToken("d".into()));
    results.cursors_closed();
    assert_eq!(results.get(kept).unwrap().more, More::Rerun, "a query is run again for it");
}

#[test]
fn a_column_s_class_comes_from_its_type_or_its_values() {
    let c = |t: &str| Column { name: "c".into(), type_name: t.into() };
    assert_eq!(class_of(&c("INTEGER"), None), CellClass::Integer);
    assert_eq!(class_of(&c("int8"), None), CellClass::Integer);
    assert_eq!(class_of(&c("numeric(10,2)"), None), CellClass::Decimal);
    assert_eq!(class_of(&c("timestamptz"), None), CellClass::DateTime);
    assert_eq!(class_of(&c("jsonb"), None), CellClass::Json);
    assert_eq!(class_of(&c("bytea"), None), CellClass::Binary);
    assert_eq!(class_of(&c("varchar(255)"), None), CellClass::Text);
    assert_eq!(class_of(&c(""), Some(&Value::Float(1.5))), CellClass::Decimal);
    assert_eq!(class_of(&c(""), Some(&Value::Int(1))), CellClass::Integer);
    assert_eq!(class_of(&c(""), None), CellClass::Text);
    let columns = grid_columns(&[c("INTEGER")], None);
    assert!(!columns[0].editable, "a result is read-only here");
}
