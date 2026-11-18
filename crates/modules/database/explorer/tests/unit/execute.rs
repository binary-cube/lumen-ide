use database::sqlite::{FOREIGN_KEYS_ON, RebuildSource, SqliteTable};
use database::{
    CellChange, Change, ColumnDef, DataSourceId, Driver, Filter, RowKey, Secret, Target,
};

use super::*;
use crate::fake::{FakeDb, FakeDriver};

fn users() -> TableRef {
    TableRef::new("users")
}

fn open(driver: &FakeDriver) -> Box<dyn Session> {
    driver.connect(&Target::default(), &Secret::default()).unwrap()
}

fn mixed() -> ChangeSet {
    ChangeSet {
        table: users(),
        changes: vec![
            Change::Insert { values: vec![("name".into(), Value::Text("new".into()))] },
            Change::Update {
                key: RowKey::RowId(2),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("user 2".into()),
                    new: Value::Text("two".into()),
                }],
            },
            Change::Delete { key: RowKey::RowId(3) },
        ],
    }
}

fn log(driver: &FakeDriver) -> Vec<String> {
    driver.db.lock().unwrap().log.clone()
}

#[test]
fn changes_run_as_the_dialect_writes_them_in_one_transaction() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let result = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    assert_eq!(result.map(|r| matches!(r, Response::Applied(a) if a.affected == 3)), Ok(true));
    // What runs is what the preview shows.
    let structure = driver.db.lock().unwrap().tables[0].structure.clone();
    let script = apply_script(&database::sqlite::Sqlite, &mixed(), Some(&structure)).unwrap();
    assert_eq!(log(&driver), script.statements());
    assert!(log(&driver)[1].starts_with("DELETE"), "deletes first");
    assert_eq!(driver.db.lock().unwrap().committed.len(), 3);
}

#[test]
fn a_row_changed_since_it_was_read_is_a_conflict_and_nothing_is_applied() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    driver.db.lock().unwrap().affected = vec![("UPDATE".into(), 0)];
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let result = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    assert!(matches!(result, Err(DbError::Conflict(_))), "{result:?}");
    let log = log(&driver);
    let kinds: Vec<&str> = log.iter().map(|q| q.split(' ').next().unwrap()).collect();
    // The insert never ran: rolled back at the update.
    assert_eq!(kinds, vec!["BEGIN", "DELETE", "UPDATE", "ROLLBACK"]);
    assert!(driver.db.lock().unwrap().committed.is_empty());
    assert!(driver.db.lock().unwrap().open.is_none());
}

#[test]
fn a_key_finding_several_rows_is_a_conflict_too() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    driver.db.lock().unwrap().affected = vec![("DELETE".into(), 2)];
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let result = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    assert!(matches!(result, Err(DbError::Conflict(_))));
    assert_eq!(log(&driver).last().map(String::as_str), Some("ROLLBACK"));
}

#[test]
fn a_failing_statement_or_a_cancel_rolls_everything_back() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    let mut session = open(&driver);
    let script = Script {
        before: Vec::new(),
        begin: "BEGIN".into(),
        steps: vec![
            database::Step::any("INSERT 1"),
            database::Step::any("FAIL"),
            database::Step::any("INSERT 2"),
        ],
        commit: "COMMIT".into(),
        rollback: "ROLLBACK".into(),
        after: vec!["AFTER".into()],
        stale: Vec::new(),
    };
    assert_eq!(
        run_script(session.as_mut(), &script, &Cancel::default()),
        Err(DbError::Query("refused".into()))
    );
    assert_eq!(log(&driver), vec!["BEGIN", "INSERT 1", "FAIL", "ROLLBACK", "AFTER"]);

    driver.db.lock().unwrap().log.clear();
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(run_script(session.as_mut(), &script, &cancel), Err(DbError::Cancelled));
    assert_eq!(log(&driver), vec!["BEGIN", "ROLLBACK", "AFTER"]);
    assert!(driver.db.lock().unwrap().committed.is_empty());
}

#[test]
fn a_rebuild_runs_its_steps_in_order() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    driver.db.lock().unwrap().foreign_keys = true;
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let changes = vec![SchemaChange::AlterColumn {
        table: users(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    }];
    let result =
        run(session.as_mut(), &Request::Alter(changes.clone()), &source, &Cancel::default());
    assert!(result.is_ok(), "{result:?}");
    let structure = driver.db.lock().unwrap().tables[0].structure.clone();
    let rebuild = RebuildSource {
        table: SqliteTable { structure, ..SqliteTable::default() },
        foreign_keys: true,
    };
    let script = database::transaction::rebuild_script(&rebuild, &changes).unwrap();
    assert_eq!(log(&driver), script.statements());
    assert_eq!(log(&driver).first().map(String::as_str), Some("PRAGMA foreign_keys = OFF"));
}

#[test]
fn a_rebuild_breaking_a_foreign_key_is_rolled_back_and_foreign_keys_come_back_on() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    {
        let mut db = driver.db.lock().unwrap();
        db.foreign_keys = true;
        // Broken before: one row; after: two.
        let broken = |rowid: i64| {
            vec![
                Value::Text("users".into()),
                Value::Int(rowid),
                Value::Text("teams".into()),
                Value::Int(0),
            ]
        };
        db.results = [
            ("PRAGMA foreign_key_check".into(), vec![broken(1)]),
            ("PRAGMA foreign_key_check".into(), vec![broken(1), broken(2)]),
        ]
        .into();
    }
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let changes = vec![SchemaChange::AlterColumn {
        table: users(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    }];
    let result = run(session.as_mut(), &Request::Alter(changes), &source, &Cancel::default());
    assert!(matches!(result, Err(DbError::Query(_))), "{result:?}");
    let log = log(&driver);
    assert_eq!(log[log.len() - 2], "ROLLBACK");
    assert_eq!(log[log.len() - 1], FOREIGN_KEYS_ON);
    assert!(driver.db.lock().unwrap().committed.is_empty());
}

#[test]
fn foreign_keys_broken_before_a_rebuild_dont_stop_it() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    {
        let mut db = driver.db.lock().unwrap();
        db.foreign_keys = true;
        let broken = vec![
            Value::Text("users".into()),
            Value::Int(3),
            Value::Text("teams".into()),
            Value::Int(0),
        ];
        db.results = [
            ("PRAGMA foreign_key_check".into(), vec![broken.clone()]),
            ("PRAGMA foreign_key_check".into(), vec![broken]),
        ]
        .into();
    }
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let changes = vec![SchemaChange::AlterColumn {
        table: users(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    }];
    let result = run(session.as_mut(), &Request::Alter(changes), &source, &Cancel::default());
    assert!(result.is_ok(), "{result:?}");
    assert!(log(&driver).contains(&"COMMIT".to_owned()));
}

#[test]
fn a_column_changed_only_in_its_name_is_renamed_without_a_rebuild() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let changes = vec![SchemaChange::AlterColumn {
        table: users(),
        column: "name".into(),
        to: ColumnDef::new("title", "TEXT"),
    }];
    let result = run(session.as_mut(), &Request::Alter(changes), &source, &Cancel::default());
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        log(&driver),
        vec![
            "BEGIN IMMEDIATE",
            "ALTER TABLE \"users\" RENAME COLUMN \"name\" TO \"title\"",
            "COMMIT"
        ]
    );
}

#[test]
fn inserted_rows_give_back_their_keys() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    driver.db.lock().unwrap().results = [("INSERT".into(), vec![vec![Value::Int(6)]])].into();
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let result = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    let Ok(Response::Applied(applied)) = result else { panic!("{result:?}") };
    assert_eq!(applied.inserted, vec![RowKey::Primary(vec![("id".into(), Value::Int(6))])]);
    assert!(log(&driver).iter().any(|s| s.ends_with("RETURNING \"id\"")));
}

#[test]
fn what_alter_table_can_do_is_plain_ddl() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let changes = vec![SchemaChange::RenameColumn {
        table: users(),
        from: "name".into(),
        to: "title".into(),
    }];
    let result = run(session.as_mut(), &Request::Alter(changes), &source, &Cancel::default());
    assert_eq!(result, Ok(Response::Applied(Applied { affected: 1, inserted: Vec::new() })));
    assert_eq!(
        log(&driver),
        vec![
            "BEGIN IMMEDIATE",
            "ALTER TABLE \"users\" RENAME COLUMN \"name\" TO \"title\"",
            "COMMIT"
        ]
    );
}

#[test]
fn a_read_only_source_writes_nothing() {
    let driver = FakeDriver::new(FakeDb::users(5), true);
    let source = Source { read_only: true, ..driver.source(DataSourceId::new()) };
    let mut session = open(&driver);
    let apply = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    assert!(matches!(apply, Err(DbError::ReadOnly(_))));
    let alter = run(
        session.as_mut(),
        &Request::Alter(vec![SchemaChange::DropTable { table: users() }]),
        &source,
        &Cancel::default(),
    );
    assert!(matches!(alter, Err(DbError::ReadOnly(_))));
    assert!(log(&driver).is_empty());
}

#[test]
fn a_source_without_sql_applies_and_counts_by_itself() {
    let driver = FakeDriver::new(FakeDb::users(5), false);
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let result = run(session.as_mut(), &Request::Apply(mixed()), &source, &Cancel::default());
    assert!(result.is_ok());
    assert_eq!(driver.db.lock().unwrap().applied, vec![mixed()]);
    let filter = Some(Filter::Contains { column: "name".into(), text: "USER 1".into() });
    let count = run(
        session.as_mut(),
        &Request::Count { table: users(), filter },
        &source,
        &Cancel::default(),
    );
    assert_eq!(count, Ok(Response::Count(1)));
    assert!(log(&driver).is_empty(), "no SQL for it");
}

#[test]
fn an_sql_source_counts_with_its_dialect() {
    let driver = FakeDriver::new(FakeDb::users(7), true);
    let source = driver.source(DataSourceId::new());
    let mut session = open(&driver);
    let count = run(
        session.as_mut(),
        &Request::Count { table: users(), filter: None },
        &source,
        &Cancel::default(),
    );
    assert_eq!(count, Ok(Response::Count(7)));
    assert_eq!(log(&driver), vec!["SELECT COUNT(*) FROM \"users\""]);
}

#[test]
fn a_commit_names_the_tables_it_changed() {
    assert_eq!(changed_tables(&Request::Apply(mixed())), vec![(users(), false)]);
    let alter = Request::Alter(vec![
        SchemaChange::DropColumn { table: users(), column: "name".into() },
        SchemaChange::RenameTable { table: users(), to: "people".into() },
    ]);
    assert_eq!(changed_tables(&alter), vec![(users(), true), (TableRef::new("people"), true)]);
    assert!(changed_tables(&Request::Structure(users())).is_empty());
}
