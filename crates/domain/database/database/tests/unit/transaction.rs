use super::*;
use crate::sqlite::{FOREIGN_KEY_CHECK, Sqlite, SqliteTable};
use crate::{CellChange, ColumnDef, ColumnInfo, GenericSql, RowKey, SchemaObject, TableDef, Value};

fn t() -> TableRef {
    TableRef::new("users")
}

fn update(id: i64) -> Change {
    Change::Update {
        key: RowKey::RowId(id),
        cells: vec![CellChange {
            column: "name".into(),
            old: Value::Text("a".into()),
            new: Value::Text("b".into()),
        }],
    }
}

fn insert(name: &str) -> Change {
    Change::Insert { values: vec![("name".into(), Value::Text(name.into()))] }
}

fn delete(id: i64) -> Change {
    Change::Delete { key: RowKey::RowId(id) }
}

/// `users (id INTEGER PRIMARY KEY, name TEXT)`, with a trigger.
fn users() -> Structure {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    id.nullable = false;
    Structure {
        table: t(),
        columns: vec![id, ColumnInfo::new("name", "TEXT")],
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        rowid: true,
        lines: false,
    }
}

fn outcome(affected: Option<u64>, rows: u64) -> Outcome {
    Outcome { affected, rows, stopped: false }
}

#[test]
fn deletes_go_first_then_updates_then_inserts_each_in_the_users_order() {
    let mut set = ChangeSet::new(t());
    set.changes = vec![insert("x"), update(1), delete(7), insert("y"), delete(3), update(2)];
    let ordered = ordered(&set);
    assert_eq!(
        ordered.changes,
        vec![delete(7), delete(3), update(1), update(2), insert("x"), insert("y")]
    );
    assert_eq!(ordered.table, t());
}

#[test]
fn an_update_or_a_delete_must_change_exactly_one_row() {
    let one = Step { sql: "UPDATE".into(), expect: Expect::OneRow };
    assert_eq!(one.check(&outcome(Some(1), 0)), Ok(()));
    assert!(matches!(one.check(&outcome(Some(0), 0)), Err(DbError::Conflict(_))));
    assert!(matches!(one.check(&outcome(Some(2), 0)), Err(DbError::Conflict(_))));
    // An engine that doesn't say can't prove it: refused rather than guessed.
    assert!(matches!(one.check(&outcome(None, 0)), Err(DbError::Conflict(_))));
    assert_eq!(Step::any("INSERT").check(&outcome(Some(5), 0)), Ok(()));
    let check = Step { sql: "PRAGMA".into(), expect: Expect::NoRows };
    assert_eq!(check.check(&outcome(None, 0)), Ok(()));
    assert!(matches!(check.check(&outcome(None, 2)), Err(DbError::Query(_))));
}

#[test]
fn the_apply_script_is_one_transaction_in_the_applied_order() {
    let mut set = ChangeSet::new(t());
    set.changes = vec![insert("x"), update(1), delete(7)];
    let script = apply_script(&Sqlite, &set, None).unwrap();
    assert_eq!(script.begin, "BEGIN IMMEDIATE");
    assert_eq!(script.commit, "COMMIT");
    assert_eq!(script.rollback, "ROLLBACK");
    let expects: Vec<Expect> = script.steps.iter().map(|s| s.expect.clone()).collect();
    assert_eq!(expects, vec![Expect::OneRow, Expect::OneRow, Expect::Any]);
    assert_eq!(script.steps[0].sql, "DELETE FROM \"users\" WHERE rowid = 7");
    assert!(script.steps[1].sql.starts_with("UPDATE \"users\" SET \"name\" = 'b'"));
    assert!(script.steps[2].sql.starts_with("INSERT INTO \"users\""));
    let all = script.statements();
    assert_eq!(all.first().map(String::as_str), Some("BEGIN IMMEDIATE"));
    assert_eq!(all.last().map(String::as_str), Some("COMMIT"));
    assert_eq!(all.len(), 5);
}

#[test]
fn a_change_the_dialect_cant_write_fails_the_whole_script() {
    let mut set = ChangeSet::new(t());
    set.changes =
        vec![insert("x"), Change::Delete { key: RowKey::Line { line: 1, fingerprint: 2 } }];
    assert!(apply_script(&GenericSql, &set, None).is_err());
}

#[test]
fn only_a_table_with_a_key_of_an_editing_driver_is_edited() {
    let edits = Capabilities { edit_rows: true, ..Capabilities::default() };
    assert_eq!(check_editable(&users(), &edits, false), Ok(()));
    assert!(matches!(check_editable(&users(), &edits, true), Err(DbError::ReadOnly(_))));
    assert!(matches!(
        check_editable(&users(), &Capabilities::default(), false),
        Err(DbError::ReadOnly(_))
    ));
    let mut keyless = users();
    keyless.rowid = false;
    keyless.columns[0].in_key = false;
    assert!(matches!(check_editable(&keyless, &edits, false), Err(DbError::ReadOnly(_))));
}

#[test]
fn a_rebuild_is_for_one_existing_table() {
    let add = SchemaChange::AddColumn { table: t(), column: ColumnDef::new("age", "INTEGER") };
    let other = SchemaChange::DropColumn { table: TableRef::new("teams"), column: "name".into() };
    let create = SchemaChange::CreateTable(TableDef {
        table: t(),
        columns: vec![ColumnDef::new("id", "INTEGER")],
        primary_key: Vec::new(),
        foreign_keys: Vec::new(),
        ..TableDef::default()
    });
    assert_eq!(alter_target(std::slice::from_ref(&add)), Some(&t()));
    assert_eq!(alter_target(&[add, other]), None);
    assert_eq!(alter_target(&[create]), None);
    assert_eq!(alter_target(&[]), None);
}

#[test]
fn only_sqlite_rebuilds_and_only_for_what_its_alter_table_cant_do() {
    let alter = SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    };
    let rename = SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "title".into() };
    assert!(needs_rebuild(&Sqlite, std::slice::from_ref(&alter), &users()));
    assert!(needs_rebuild(&Sqlite, &[rename.clone(), alter.clone()], &users()));
    assert!(!needs_rebuild(&Sqlite, std::slice::from_ref(&rename), &users()));
    assert!(!needs_rebuild(&GenericSql, &[alter], &users()));
}

#[test]
fn plain_ddl_runs_in_one_transaction_in_order() {
    let changes = [
        SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "title".into() },
        SchemaChange::AddColumn { table: t(), column: ColumnDef::new("age", "INTEGER") },
    ];
    let script = alter_script(&Sqlite, &changes).unwrap();
    let sql: Vec<&str> = script.steps.iter().map(|s| s.sql.as_str()).collect();
    assert_eq!(
        sql,
        vec![
            "ALTER TABLE \"users\" RENAME COLUMN \"name\" TO \"title\"",
            "ALTER TABLE \"users\" ADD COLUMN \"age\" INTEGER",
        ]
    );
    assert!(script.steps.iter().all(|s| s.expect == Expect::Any));
    assert!(script.before.is_empty() && script.after.is_empty());
}

#[test]
fn an_insert_gives_back_its_key_when_the_structure_is_known() {
    let mut set = ChangeSet::new(t());
    set.changes = vec![insert("x"), delete(7)];
    let script = apply_script(&Sqlite, &set, Some(&users())).unwrap();
    let key = KeyKind::Primary(vec!["id".into()]);
    assert_eq!(script.steps[1].expect, Expect::Inserted(key));
    assert_eq!(
        script.steps[1].sql,
        "INSERT INTO \"users\" (\"name\") VALUES ('x') RETURNING \"id\""
    );
    let row = vec![vec![Value::Int(9)]];
    assert_eq!(
        script.steps[1].verify(&outcome(Some(1), 1), &row, None),
        Ok(Some(RowKey::Primary(vec![("id".into(), Value::Int(9))])))
    );
    let by_rowid = Step { sql: "INSERT".into(), expect: Expect::Inserted(KeyKind::RowId) };
    assert_eq!(by_rowid.verify(&outcome(Some(1), 1), &row, None), Ok(Some(RowKey::RowId(9))));
    // Generic SQL returns nothing: a plain insert.
    let script = apply_script(
        &GenericSql,
        &ChangeSet { table: t(), changes: vec![insert("x")] },
        Some(&users()),
    )
    .unwrap();
    assert_eq!(script.steps[0].expect, Expect::Any);
}

#[test]
fn only_new_foreign_key_violations_of_the_tables_changed_count() {
    let row = |table: &str, rowid: i64, parent: &str| {
        vec![
            Value::Text(table.into()),
            Value::Int(rowid),
            Value::Text(parent.into()),
            Value::Int(0),
        ]
    };
    let before = vec![row("users", 1, "teams"), row("orders", 4, "items")];
    // The same rows broken (one renumbered by a rebuild), one more of another table: none new.
    let after =
        vec![row("users", 7, "teams"), row("orders", 4, "items"), row("orders", 5, "items")];
    let tables = vec!["Users".to_owned()];
    assert_eq!(new_violations(&before, &after, &tables), 0);
    let worse = vec![row("users", 7, "teams"), row("users", 8, "teams")];
    assert_eq!(new_violations(&before, &worse, &tables), 1);
    // Pointing to a table changed counts too.
    let pointing = vec![row("users", 1, "teams"), row("orders", 4, "users")];
    assert_eq!(new_violations(&before, &pointing, &tables), 1);

    let check = Step { sql: FOREIGN_KEY_CHECK.into(), expect: Expect::NoNewViolations { tables } };
    assert_eq!(check.verify(&outcome(None, 3), &after, Some(&before)), Ok(None));
    assert!(matches!(
        check.verify(&outcome(None, 2), &worse, Some(&before)),
        Err(DbError::Query(_))
    ));
    assert!(
        check.verify(&outcome(None, 1), &[row("users", 1, "teams")], None).is_err(),
        "no baseline: all new"
    );
}

fn sqlite_users(triggers: Vec<SchemaObject>) -> SqliteTable {
    SqliteTable { structure: users(), triggers, ..SqliteTable::default() }
}

#[test]
fn the_rebuild_script_runs_the_plans_steps_in_order() {
    let table = sqlite_users(vec![SchemaObject {
        name: "touch".into(),
        sql: "CREATE TRIGGER touch AFTER UPDATE ON users BEGIN SELECT 1; END".into(),
    }]);
    let changes = [SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    }];
    let plan = crate::sqlite::rebuild(&table, &changes).unwrap();
    for foreign_keys in [true, false] {
        let source = RebuildSource { table: table.clone(), foreign_keys };
        let script = rebuild_script(&source, &changes).unwrap();
        let sql: Vec<String> = script.steps.iter().map(|s| s.sql.clone()).collect();
        if foreign_keys {
            // Off before; what's broken read first, and again before the commit; on after.
            assert_eq!(script.before, vec![FOREIGN_KEYS_OFF]);
            assert_eq!(script.after, vec![FOREIGN_KEYS_ON]);
            assert_eq!(
                script.steps[0],
                Step { sql: FOREIGN_KEY_CHECK.into(), expect: Expect::Baseline }
            );
            assert_eq!(
                script.steps.last(),
                Some(&Step {
                    sql: FOREIGN_KEY_CHECK.into(),
                    expect: Expect::NoNewViolations { tables: vec!["users".into()] }
                })
            );
            assert_eq!(sql[1..sql.len() - 1], plan.statements[..]);
        } else {
            assert!(script.before.is_empty() && script.after.is_empty());
            assert_eq!(sql, plan.statements);
        }
        assert!(script.stale.is_empty());
    }
}

#[test]
fn several_tables_rebuilt_in_one_transaction_each_schema_checked() {
    let mut teams = users();
    teams.table = TableRef::in_schema("aux", "teams");
    let alter = |table: TableRef| SchemaChange::AlterColumn {
        table,
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT").not_null(),
    };
    let add = SchemaChange::AddColumn {
        table: TableRef::new("log"),
        column: ColumnDef::new("at", "TEXT"),
    };
    let parts = vec![
        AlterPart::Rebuild(
            Box::new(RebuildSource { table: sqlite_users(Vec::new()), foreign_keys: true }),
            vec![alter(t())],
        ),
        AlterPart::Plain(vec![add]),
        AlterPart::Rebuild(
            Box::new(RebuildSource {
                table: SqliteTable { structure: teams.clone(), ..SqliteTable::default() },
                foreign_keys: true,
            }),
            vec![alter(teams.table)],
        ),
    ];
    let script = parts_script(&Sqlite, parts).unwrap();
    let sql: Vec<&str> = script.steps.iter().map(|s| s.sql.as_str()).collect();
    assert_eq!(sql[0], "PRAGMA foreign_key_check");
    assert_eq!(sql[1], "PRAGMA \"aux\".foreign_key_check");
    assert!(sql.contains(&"ALTER TABLE \"log\" ADD COLUMN \"at\" TEXT"));
    assert!(sql.contains(&"DROP TABLE \"aux\".\"teams\""));
    let users_at = sql.iter().position(|s| *s == "DROP TABLE \"users\"").unwrap();
    let log_at = sql.iter().position(|s| s.starts_with("ALTER TABLE \"log\"")).unwrap();
    assert!(users_at < log_at, "the parts in order");
    assert_eq!(
        script.steps[script.steps.len() - 2].expect,
        Expect::NoNewViolations { tables: vec!["users".into(), "log".into()] }
    );
    assert_eq!(
        script.steps[script.steps.len() - 1].expect,
        Expect::NoNewViolations { tables: vec!["teams".into()] }
    );
    assert_eq!(script.before, vec![FOREIGN_KEYS_OFF]);
}

#[test]
fn a_column_a_view_names_is_dropped_by_a_rebuild() {
    let source = RebuildSource {
        table: SqliteTable {
            structure: users(),
            views: vec![SchemaObject {
                name: "v".into(),
                sql: "CREATE VIEW v AS SELECT name FROM users".into(),
            }],
            ..SqliteTable::default()
        },
        foreign_keys: false,
    };
    let drop = |column: &str| [SchemaChange::DropColumn { table: t(), column: column.into() }];
    assert!(drops_named(&source, &drop("name")));
    assert!(!drops_named(&source, &drop("id")));
}

#[test]
fn a_transaction_without_words_to_begin_it_shows_and_runs_only_its_steps() {
    let script = Script {
        before: Vec::new(),
        begin: String::new(),
        steps: vec![Step::any("SET users.name = text")],
        commit: String::new(),
        rollback: String::new(),
        after: Vec::new(),
        stale: Vec::new(),
    };
    assert_eq!(script.statements(), vec!["SET users.name = text".to_owned()]);
}

#[test]
fn changes_inside_an_open_transaction_run_between_a_savepoint_and_its_release() {
    let set = ChangeSet { table: t(), changes: vec![update(1), delete(2)] };
    let script = apply_script(&Sqlite, &set, None).unwrap();
    let inner = nested(&script, "console_edit");
    assert_eq!(inner.steps, script.steps, "the same steps, checked the same way");
    assert_eq!(
        inner.statements(),
        [
            "SAVEPOINT console_edit".to_owned(),
            script.steps[0].sql.clone(),
            script.steps[1].sql.clone(),
            "RELEASE SAVEPOINT console_edit".to_owned(),
        ]
    );
    assert_eq!(inner.rollback, "ROLLBACK TO SAVEPOINT console_edit");
    assert!(inner.before.is_empty() && inner.after.is_empty());
}
