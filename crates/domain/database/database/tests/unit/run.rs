use std::sync::Arc;

use super::*;
use crate::sql::EnginePosition;
use crate::{
    Capabilities, Column, ColumnInfo, ConsoleCaps, Dialect, Isolations, Node, Outcome,
    SchemaSwitch, SqlError, Structure, TableInfo, TableKind,
};

/// A session whose `execute` gives `rows` rows of one column (or two sets, when asked) and
/// keeps what it ran.
#[derive(Default)]
struct Rows {
    rows: u64,
    sets: u64,
    ran: Vec<String>,
    /// The rows read before it stopped.
    read: u64,
    dialect: Option<Arc<dyn Dialect>>,
}

impl Session for Rows {
    fn server(&self) -> String {
        "rows".into()
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        self.dialect.clone()
    }

    fn children(&mut self, _: &[String]) -> Result<Vec<Node>, DbError> {
        Ok(Vec::new())
    }

    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        _: &Cancel,
    ) -> Result<Outcome, DbError> {
        self.ran.push(query.to_owned());
        let mut outcome = Outcome::default();
        for _ in 0..self.sets.max(1) {
            sink.columns(&[Column { name: "n".into(), type_name: "int".into() }]);
            for n in 0..self.rows {
                outcome.rows += 1;
                self.read += 1;
                sink.row_key(RowKey::RowId(n as i64));
                if sink.row(vec![Value::Int(n as i64)]) == Flow::Stop {
                    outcome.stopped = true;
                    return Ok(outcome);
                }
            }
        }
        outcome.affected = Some(0);
        Ok(outcome)
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        Ok(["a", "b"]
            .iter()
            .map(|name| TableInfo {
                table: TableRef { schema: path.first().cloned(), name: (*name).into() },
                kind: TableKind::Table,
                rows: None,
            })
            .collect())
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        let mut id = ColumnInfo::new("id", "INTEGER");
        id.in_key = true;
        id.key_position = Some(1);
        id.nullable = false;
        let mut name = ColumnInfo::new(format!("{}_name", table.name), "TEXT");
        name.default = Some("'x'".into());
        Ok(Structure { table: table.clone(), columns: vec![id, name], ..Structure::default() })
    }
}

#[test]
fn the_default_run_streams_a_page_and_says_more_are_left() {
    let mut session = Rows { rows: 1_000, ..Rows::default() };
    let mut sink = Collect::default();
    let options = RunOptions { fetch_limit: 10, keep_cursor: true };
    let outcome = session.run("SELECT n FROM t", &options, &mut sink, &Cancel::default()).unwrap();
    assert_eq!(sink.rows.len(), 10);
    assert_eq!(sink.keys.len(), 10, "the keys of the rows passed on only");
    assert_eq!(outcome.rows, 10);
    assert!(outcome.truncated && !outcome.stopped && outcome.more.is_none());
    assert_eq!(session.read, 11, "one row past the limit read, then stopped");
    // Exactly the limit: nothing more.
    let mut session = Rows { rows: 10, ..Rows::default() };
    let outcome = session.run("x", &options, &mut Collect::default(), &Cancel::default()).unwrap();
    assert!(!outcome.truncated && outcome.rows == 10 && outcome.affected == Some(0));
    // The sink stopping it is a stop, not a truncation.
    let mut session = Rows { rows: 100, ..Rows::default() };
    let mut few = Collect { limit: Some(3), ..Collect::default() };
    let outcome = session.run("x", &options, &mut few, &Cancel::default()).unwrap();
    assert!(outcome.stopped && !outcome.truncated && outcome.rows == 3);
    // And the default's defaults: no cursor, no plan, no schema switch.
    let token = CursorToken("c".into());
    let fetched = session.fetch(&token, 10, &mut Collect::default(), &Cancel::default());
    assert!(matches!(fetched, Err(DbError::Unsupported(_))));
    assert_eq!(session.close_cursor(&token), Ok(()));
    assert!(matches!(
        session.explain("x", false, &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
    assert!(matches!(session.set_schema("s"), Err(DbError::Unsupported(_))));
    assert_eq!(session.current(), Ok(Place::default()));
    assert_eq!(session.databases(), Ok(Vec::new()));
    assert!(matches!(session.routines(None, &Cancel::default()), Err(DbError::Unsupported(_))));
    assert!(!session.in_transaction());
    assert_eq!(session.console(), None, "as its driver says");
}

#[test]
fn each_result_set_has_its_own_limit_and_a_write_runs_to_its_end() {
    let mut sink = Collect::default();
    let mut limited = Limited::new(&mut sink, 2, Overflow::Drop);
    for _ in 0..2 {
        limited.columns(&[]);
        for n in 0..5 {
            assert_eq!(limited.row(vec![Value::Int(n)]), Flow::Continue, "dropped, not stopped");
        }
    }
    assert_eq!((limited.delivered, limited.truncated, limited.stopped), (4, true, false));
    assert_eq!(sink.rows.len(), 2, "the second set's two");
}

#[test]
fn transactions_by_default_are_the_dialects_words() {
    let mut session = Rows { dialect: Some(Arc::new(crate::sqlite::Sqlite)), ..Rows::default() };
    session.begin(&TxOptions::default()).unwrap();
    session.commit().unwrap();
    session.rollback().unwrap();
    let immediate = TxOptions { isolation: Some(Isolation::Exclusive), read_only: false };
    session.begin(&immediate).unwrap();
    assert_eq!(session.ran, ["BEGIN IMMEDIATE", "COMMIT", "ROLLBACK", "BEGIN EXCLUSIVE"]);
    let mut generic = Rows::default();
    generic.begin(&TxOptions::default()).unwrap();
    assert_eq!(generic.ran, ["BEGIN"]);
}

#[test]
fn a_begin_says_its_isolation_and_read_only() {
    let begin =
        |isolation, read_only| begin_statement("BEGIN", &TxOptions { isolation, read_only });
    assert_eq!(begin(None, false), "BEGIN");
    assert_eq!(begin(None, true), "START TRANSACTION READ ONLY");
    assert_eq!(
        begin(Some(Isolation::Serializable), true),
        "START TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY"
    );
    assert_eq!(
        begin(Some(Isolation::ReadCommitted), false),
        "START TRANSACTION ISOLATION LEVEL READ COMMITTED"
    );
    assert_eq!(begin(Some(Isolation::Deferred), false), "BEGIN DEFERRED");
}

#[test]
fn the_catalog_by_default_is_each_tables_structure() {
    let mut session = Rows::default();
    let all = session.catalog_columns(Some("main"), &Cancel::default()).unwrap();
    assert_eq!(all.len(), 2);
    let (table, columns) = &all[1];
    assert_eq!(table, &TableRef::in_schema("main", "b"));
    assert_eq!(columns[0].key_position, Some(1));
    assert!(!columns[0].nullable);
    assert_eq!(columns[1].name, "b_name");
    assert_eq!(columns[1].default.as_deref(), Some("'x'"));
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(session.catalog_columns(None, &cancel), Err(DbError::Cancelled));
}

#[test]
fn a_transaction_is_followed_through_the_statements_run() {
    let pg = Flavor::Postgres { version: Some(170_000) };
    assert!(transaction_after("BEGIN", pg, false));
    assert!(transaction_after("start transaction read only;", pg, false));
    assert!(!transaction_after("BEGIN; UPDATE t SET a = 1; COMMIT", pg, false));
    assert!(transaction_after("SELECT 1", pg, true), "unchanged");
    assert!(transaction_after("ROLLBACK TO SAVEPOINT a", pg, true), "still open");
    assert!(!transaction_after("END", pg, true));
    assert!(!transaction_after("abort", pg, true));
    // A body's words aren't the session's.
    assert!(!transaction_after(
        "CREATE FUNCTION f() RETURNS void AS $$ BEGIN COMMIT; END $$ LANGUAGE plpgsql",
        pg,
        false
    ));
    assert!(!transaction_after("-- BEGIN\nSELECT 'BEGIN'", Flavor::Sqlite, false));
}

#[test]
fn isolations_are_a_set_and_known_by_their_ids() {
    let pg = Isolations::of(&[Isolation::ReadCommitted, Isolation::Serializable]);
    assert!(pg.contains(Isolation::Serializable) && !pg.contains(Isolation::Deferred));
    assert_eq!(pg.iter().collect::<Vec<_>>(), [Isolation::ReadCommitted, Isolation::Serializable]);
    assert!(Isolations::NONE.is_empty());
    assert_eq!(pg, pg.iter().collect::<Isolations>());
    for isolation in Isolation::ALL {
        assert_eq!(Isolation::from_id(isolation.id()), Some(isolation));
    }
    assert_eq!(Isolation::from_id("chaos"), None);
}

#[test]
fn a_driver_without_a_console_says_none_and_an_old_one_limited() {
    assert_eq!(Capabilities::default().console, None);
    const { assert!(ConsoleCaps::LIMITED.limited && !ConsoleCaps::BASIC.limited) };
    assert!(!ConsoleCaps::BASIC.cursors && ConsoleCaps::BASIC.isolation.is_empty());
    assert_eq!(ConsoleCaps::BASIC.switch_schema, SchemaSwitch::None);
}

#[test]
fn an_sql_error_reads_as_a_query_error_did() {
    let error = DbError::sql(SqlError {
        message: "relation \"nope\" does not exist".into(),
        sqlstate: Some("42P01".into()),
        position: Some(EnginePosition::Character(15)),
        detail: None,
        hint: Some("check the name".into()),
    });
    assert_eq!(error.to_string(), "relation \"nope\" does not exist");
    assert_eq!(error.sql_error().and_then(|e| e.sqlstate.as_deref()), Some("42P01"));
    let detailed = SqlError { detail: Some("Key (id)=(1) exists.".into()), ..SqlError::new("dup") };
    assert_eq!(DbError::sql(detailed).to_string(), "dup (Key (id)=(1) exists.)");
    assert_eq!(DbError::Query("x".into()).sql_error(), None);
}

/// A column read with its table keeps its enum's labels as completion's brief (48h).
#[test]
fn a_structure_s_column_keeps_its_values_in_the_brief() {
    let mut status = crate::ColumnInfo::new("status", "order_status");
    status.values = Some(vec!["new".into(), "paid".into()]);
    let brief = ColumnBrief::from(&status);
    assert_eq!(brief.values, Some(vec!["new".into(), "paid".into()]));
    assert_eq!(ColumnBrief::from(&crate::ColumnInfo::new("id", "int")).values, None);
}
