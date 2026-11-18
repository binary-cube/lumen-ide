use super::*;

#[test]
fn values_show_as_written_and_secrets_never() {
    assert_eq!(Value::Null.display(), "");
    assert_eq!(Value::Int(42).display(), "42");
    assert_eq!(Value::Bytes(vec![0; 3]).display(), "(3 bytes)");
    assert_eq!(format!("{:?}", Secret::new("s3cret")), "Secret(…)");
}

#[test]
fn a_sink_with_a_limit_stops_the_rows() {
    let mut sink = Collect { limit: Some(2), ..Collect::default() };
    sink.columns(&[Column { name: "id".into(), type_name: "int".into() }]);
    assert_eq!(sink.row(vec![Value::Int(1)]), Flow::Continue);
    assert_eq!(sink.row(vec![Value::Int(2)]), Flow::Stop);
    // A second result set starts afresh.
    sink.columns(&[]);
    assert!(sink.rows.is_empty());
    let cancel = Cancel::default();
    let shared = cancel.clone();
    shared.cancel();
    assert!(cancel.is_cancelled(), "seen from another handle");
}

#[test]
fn a_session_offers_only_what_it_was_given() {
    struct Bare;
    impl Session for Bare {
        fn server(&self) -> String {
            "bare".into()
        }
        fn children(&mut self, _: &[String]) -> Result<Vec<Node>, DbError> {
            Ok(Vec::new())
        }
        fn execute(
            &mut self,
            _: &str,
            _: &mut dyn RowSink,
            _: &Cancel,
        ) -> Result<Outcome, DbError> {
            Ok(Outcome::default())
        }
    }
    let mut session = Bare;
    let table = TableRef::new("t");
    assert!(matches!(session.tables(&[]), Err(DbError::Unsupported(_))));
    assert!(matches!(session.structure(&table), Err(DbError::Unsupported(_))));
    let mut sink = Collect::default();
    let read = session.read(&table, &Page::first(10), &mut sink, &Cancel::default());
    assert!(matches!(read, Err(DbError::Unsupported(_))));
    assert!(matches!(session.apply(&ChangeSet::new(table)), Err(DbError::Unsupported(_))));
    assert!(matches!(session.alter(&[]), Err(DbError::Unsupported(_))));
}

#[test]
fn a_sink_keeps_the_rows_keys_per_result_set() {
    let mut sink = Collect::default();
    sink.columns(&[]);
    sink.row_key(RowKey::RowId(7));
    sink.row(vec![Value::Int(1)]);
    assert_eq!(sink.keys, vec![RowKey::RowId(7)]);
    sink.columns(&[]);
    assert!(sink.keys.is_empty());
}

#[test]
fn a_rows_fingerprint_is_stable_and_tells_values_apart() {
    let row = [Value::Int(1), Value::Text("a".into()), Value::Null];
    assert_eq!(fingerprint(&row), fingerprint(&row.clone()));
    // Fixed: the same in every run and on every platform (saved with a pending change).
    assert_eq!(fingerprint(&[]), 0xcbf2_9ce4_8422_2325);
    assert_ne!(fingerprint(&[Value::Text("1".into())]), fingerprint(&[Value::Int(1)]));
    assert_ne!(
        fingerprint(&[Value::Text("ab".into()), Value::Text("c".into())]),
        fingerprint(&[Value::Text("a".into()), Value::Text("bc".into())]),
        "cells don't run into each other"
    );
}

#[test]
fn sorting_at_the_source_may_have_a_size_limit() {
    assert!(!Sorting::None.allows(0));
    assert!(Sorting::Any.allows(u64::MAX));
    assert!(Sorting::UpTo(10).allows(10) && !Sorting::UpTo(10).allows(11));
    assert_eq!(Capabilities::default().sorting, Sorting::None);
    let held = DbError::InUse("people.csv".into());
    assert_eq!(held.to_string(), "used by another program: people.csv");
}
