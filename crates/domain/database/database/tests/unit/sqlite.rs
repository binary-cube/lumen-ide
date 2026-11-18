use super::*;
use crate::{Change, ChangeSet, ColumnInfo, FkAction, Generated, RowKey};

fn t() -> TableRef {
    TableRef::new("users")
}

fn column(name: &str, declared: &str) -> ColumnInfo {
    ColumnInfo::new(name, declared)
}

/// `users (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'x', email TEXT UNIQUE,
/// team INTEGER REFERENCES teams(id))`, with an index on `name`, a trigger and a view.
fn users() -> SqliteTable {
    let mut id = column("id", "INTEGER");
    id.in_key = true;
    id.nullable = false;
    let mut name = column("name", "TEXT");
    name.nullable = false;
    name.default = Some("'x'".into());
    SqliteTable {
        structure: Structure {
            table: t(),
            columns: vec![id, name, column("email", "TEXT"), column("team", "INTEGER")],
            indexes: vec![
                IndexInfo {
                    name: "sqlite_autoindex_users_1".into(),
                    columns: vec!["email".into()],
                    unique: true,
                    implicit: true,
                    ..IndexInfo::default()
                },
                IndexInfo {
                    name: "users_name".into(),
                    columns: vec!["name".into()],
                    unique: false,
                    implicit: false,
                    ..IndexInfo::default()
                },
            ],
            foreign_keys: vec![ForeignKey {
                name: None,
                columns: vec!["team".into()],
                table: TableRef::new("teams"),
                references: vec!["id".into()],
                on_update: FkAction::NoAction,
                on_delete: FkAction::Cascade,
            }],
            rowid: true,
            lines: false,
        },
        checks: vec!["length(name) > 0".into()],
        triggers: vec![SchemaObject {
            name: "users_touch".into(),
            sql: "CREATE TRIGGER users_touch AFTER UPDATE ON users BEGIN SELECT NEW.email; END"
                .into(),
        }],
        written_indexes: Vec::new(),
        views: vec![SchemaObject {
            name: "names".into(),
            sql: "CREATE VIEW names AS SELECT name FROM users".into(),
        }],
    }
}

fn stale(kind: ObjectKind, name: &str) -> Stale {
    Stale { table: t(), kind, name: name.into() }
}

#[test]
fn sqlite_writes_booleans_as_numbers_and_finds_rows_by_rowid() {
    let d = Sqlite;
    assert_eq!(d.literal(&Value::Bool(true)).unwrap(), "1");
    assert_eq!(d.literal(&Value::Bool(false)).unwrap(), "0");
    assert_eq!(d.literal(&Value::Text("it's".into())).unwrap(), "'it''s'");
    assert_eq!(d.literal(&Value::Bytes(vec![1, 2])).unwrap(), "X'0102'");
    let mut set = ChangeSet::new(t());
    set.changes.push(Change::Update {
        key: RowKey::RowId(42),
        cells: vec![crate::CellChange {
            column: "active".into(),
            old: Value::Bool(false),
            new: Value::Bool(true),
        }],
    });
    set.changes.push(Change::Delete { key: RowKey::RowId(-1) });
    assert_eq!(
        d.changes(&set).unwrap(),
        vec![
            "UPDATE \"users\" SET \"active\" = 1 WHERE rowid = 42 AND \"active\" = 0",
            "DELETE FROM \"users\" WHERE rowid = -1",
        ]
    );
    assert_eq!(d.begin(), "BEGIN IMMEDIATE", "the write lock at once");
}

#[test]
fn sqlite_ddl_numbers_its_key_and_qualifies_indexes() {
    let d = Sqlite;
    let mut id = ColumnDef::new("id", "INTEGER");
    id.auto_increment = true;
    let create = SchemaChange::CreateTable(TableDef {
        table: TableRef::in_schema("main", "notes"),
        columns: vec![
            id.clone(),
            ColumnDef::new("body", "TEXT")
                .default(DefaultValue::Expression("datetime('now')".into())),
        ],
        primary_key: vec!["id".into()],
        foreign_keys: vec![],
        ..TableDef::default()
    });
    assert_eq!(
        d.schema(&create).unwrap(),
        vec![
            "CREATE TABLE \"main\".\"notes\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \"body\" TEXT DEFAULT (datetime('now')))"
        ]
    );
    let index = IndexInfo {
        name: "notes_body".into(),
        columns: vec!["body".into()],
        unique: false,
        implicit: false,
        ..IndexInfo::default()
    };
    assert_eq!(
        d.schema(&SchemaChange::CreateIndex { table: TableRef::in_schema("main", "notes"), index })
            .unwrap(),
        vec!["CREATE INDEX \"main\".\"notes_body\" ON \"notes\" (\"body\")"]
    );
    // SQLite numbers only an INTEGER primary key, and only one.
    let mut text_id = ColumnDef::new("id", "TEXT");
    text_id.auto_increment = true;
    let bad = SchemaChange::CreateTable(TableDef {
        table: t(),
        columns: vec![text_id],
        primary_key: vec![],
        foreign_keys: vec![],
        ..TableDef::default()
    });
    assert!(d.schema(&bad).is_err());
    let composite = SchemaChange::CreateTable(TableDef {
        table: t(),
        columns: vec![id, ColumnDef::new("b", "INTEGER")],
        primary_key: vec!["id".into(), "b".into()],
        foreign_keys: vec![],
        ..TableDef::default()
    });
    assert!(d.schema(&composite).is_err());
    // A column changes only through a rebuild.
    let alter = SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT"),
    };
    assert!(matches!(d.schema(&alter), Err(DbError::Unsupported(_))));
}

#[test]
fn what_alter_table_cant_do_needs_a_rebuild() {
    let s = users().structure;
    let drop = |c: &str| SchemaChange::DropColumn { table: t(), column: c.into() };
    assert!(needs_rebuild(&drop("id"), &s), "in the key");
    assert!(needs_rebuild(&drop("name"), &s), "indexed");
    assert!(needs_rebuild(&drop("email"), &s), "unique");
    assert!(needs_rebuild(&drop("team"), &s), "a foreign key");
    let mut plain = s.clone();
    plain.columns.push(column("note", "TEXT"));
    assert!(!needs_rebuild(&drop("note"), &plain), "DROP COLUMN does it");

    let add = |c: ColumnDef| SchemaChange::AddColumn { table: t(), column: c };
    assert!(!needs_rebuild(&add(ColumnDef::new("a", "TEXT")), &s));
    assert!(!needs_rebuild(
        &add(ColumnDef::new("a", "TEXT").not_null().default(DefaultValue::Value(Value::Int(0)))),
        &s
    ));
    assert!(needs_rebuild(&add(ColumnDef::new("a", "TEXT").not_null()), &s));
    assert!(needs_rebuild(
        &add(ColumnDef::new("a", "TEXT").default(DefaultValue::CurrentTimestamp)),
        &s
    ));
    let alter = SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("name", "TEXT"),
    };
    assert!(needs_rebuild(&alter, &s));
    let rename = SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "n".into() };
    assert!(!needs_rebuild(&rename, &s));
}

#[test]
fn a_rebuild_follows_sqlites_procedure_and_renames_last() {
    let changes = [
        SchemaChange::AlterColumn {
            table: t(),
            column: "name".into(),
            to: ColumnDef::new("full_name", "VARCHAR(200)").not_null(),
        },
        SchemaChange::DropColumn { table: t(), column: "team".into() },
        SchemaChange::AddColumn {
            table: t(),
            column: ColumnDef::new("age", "INTEGER").default(DefaultValue::Value(Value::Int(0))),
        },
    ];
    let plan = rebuild(&users(), &changes).unwrap();
    assert_eq!(
        plan.statements,
        vec![
            "DROP VIEW \"names\"",
            "CREATE TABLE \"new_users\" (\"id\" INTEGER NOT NULL, \"name\" VARCHAR(200) NOT NULL, \
             \"email\" TEXT, \"age\" INTEGER DEFAULT 0, PRIMARY KEY (\"id\"), UNIQUE (\"email\"), \
             CHECK (length(name) > 0))",
            "INSERT INTO \"new_users\" (\"id\", \"name\", \"email\") SELECT \"id\", \"name\", \"email\" FROM \"users\"",
            "DROP TABLE \"users\"",
            "ALTER TABLE \"new_users\" RENAME TO \"users\"",
            "CREATE INDEX \"users_name\" ON \"users\" (\"name\")",
            "CREATE TRIGGER users_touch AFTER UPDATE ON users BEGIN SELECT NEW.email; END",
            "CREATE VIEW names AS SELECT name FROM users",
            // Renamed last: SQLite rewrites the view, the index and the check naming it.
            "ALTER TABLE \"users\" RENAME COLUMN \"name\" TO \"full_name\"",
        ]
    );
    assert!(plan.stale.is_empty(), "the view names the column renamed, not one dropped");
}

#[test]
fn what_names_a_dropped_column_is_stale_and_not_made_again() {
    let plan = rebuild(&users(), &[SchemaChange::DropColumn { table: t(), column: "name".into() }])
        .unwrap();
    assert_eq!(
        plan.stale,
        vec![stale(ObjectKind::Index, "users_name"), stale(ObjectKind::View, "names")]
    );
    assert_eq!(
        plan.statements[1],
        "CREATE TABLE \"new_users\" (\"id\" INTEGER NOT NULL, \"email\" TEXT, \"team\" INTEGER, \
         PRIMARY KEY (\"id\"), FOREIGN KEY (\"team\") REFERENCES \"teams\" (\"id\") ON DELETE CASCADE, \
         UNIQUE (\"email\"), CHECK (length(name) > 0))"
    );
    assert!(!plan.statements.iter().any(|s| s.contains("users_name")));
    assert!(!plan.statements.iter().any(|s| s.starts_with("CREATE VIEW")), "the view goes");
    assert_eq!(plan.statements[0], "DROP VIEW \"names\"", "dropped all the same");
    assert!(plan.statements.iter().any(|s| s.starts_with("CREATE TRIGGER")), "the trigger stays");

    // A view over a view that goes goes too; a word in a string isn't a name.
    let mut table = users();
    table.views.push(SchemaObject {
        name: "names_again".into(),
        sql: "CREATE VIEW names_again AS SELECT * FROM names".into(),
    });
    table.triggers[0].sql =
        "CREATE TRIGGER users_touch AFTER UPDATE ON users BEGIN SELECT 'name'; END".into();
    let plan =
        rebuild(&table, &[SchemaChange::DropColumn { table: t(), column: "name".into() }]).unwrap();
    let names: Vec<&str> = plan.stale.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["users_name", "names", "names_again"]);
}

#[test]
fn a_rename_alone_keeps_everything_and_renames_last() {
    let plan = rebuild(
        &users(),
        &[SchemaChange::RenameColumn { table: t(), from: "email".into(), to: "mail".into() }],
    )
    .unwrap();
    // An old default (the engine's own expression) is kept, in parentheses.
    assert!(
        plan.statements[1].contains("\"name\" TEXT NOT NULL DEFAULT ('x')"),
        "{}",
        plan.statements[1]
    );
    assert!(plan.statements[1].contains("UNIQUE (\"email\")"));
    assert_eq!(
        plan.statements.last().map(String::as_str),
        Some("ALTER TABLE \"users\" RENAME COLUMN \"email\" TO \"mail\"")
    );
    assert!(plan.stale.is_empty(), "the trigger naming it is rewritten by SQLite");
}

#[test]
fn names_swapped_are_renamed_in_the_table_and_what_names_them_is_stale() {
    // `email` becomes `x`, then `name` takes `email`: renaming last would clash.
    let changes = [
        SchemaChange::RenameColumn { table: t(), from: "email".into(), to: "x".into() },
        SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "email".into() },
        SchemaChange::DropColumn { table: t(), column: "team".into() },
    ];
    let plan = rebuild(&users(), &changes).unwrap();
    assert!(plan.statements[1].contains("\"email\" TEXT NOT NULL DEFAULT ('x'), \"x\" TEXT"));
    assert!(
        plan.statements
            .contains(&"CREATE INDEX \"users_name\" ON \"users\" (\"email\")".to_owned())
    );
    assert!(!plan.statements.iter().any(|s| s.contains("RENAME COLUMN")));
    assert_eq!(
        plan.stale,
        vec![stale(ObjectKind::View, "names"), stale(ObjectKind::Trigger, "users_touch")]
    );
    let taken = [
        SchemaChange::RenameColumn { table: t(), from: "email".into(), to: "x".into() },
        SchemaChange::AddColumn { table: t(), column: ColumnDef::new("x", "TEXT") },
    ];
    assert!(matches!(rebuild(&users(), &taken), Err(DbError::Query(_))));
}

#[test]
fn indexes_as_written_are_made_again_unless_dropped_or_stale() {
    let mut table = users();
    table.written_indexes = vec![
        SchemaObject {
            name: "users_lower".into(),
            sql: "CREATE INDEX users_lower ON users (lower(email))".into(),
        },
        SchemaObject {
            name: "users_teamed".into(),
            sql: "CREATE INDEX users_teamed ON users (name) WHERE team IS NOT NULL".into(),
        },
    ];
    let alter = SchemaChange::AlterColumn {
        table: t(),
        column: "email".into(),
        to: ColumnDef::new("email", "VARCHAR(100)"),
    };
    let plan = rebuild(&table, std::slice::from_ref(&alter)).unwrap();
    assert!(
        plan.statements.contains(&"CREATE INDEX users_lower ON users (lower(email))".to_owned())
    );
    assert!(plan.statements.iter().any(|s| s.starts_with("CREATE INDEX users_teamed")));

    // Dropped: never made again.
    let drop = SchemaChange::DropIndex { table: t(), name: "users_lower".into() };
    let plan = rebuild(&table, &[drop, alter]).unwrap();
    assert!(!plan.statements.iter().any(|s| s.contains("users_lower")));
    assert!(plan.stale.is_empty(), "dropped as asked: not stale");

    // Its column dropped: stale.
    let plan =
        rebuild(&table, &[SchemaChange::DropColumn { table: t(), column: "team".into() }]).unwrap();
    assert_eq!(plan.stale, vec![stale(ObjectKind::Index, "users_teamed")]);
    assert!(!plan.statements.iter().any(|s| s.contains("users_teamed")));
}

#[test]
fn objects_of_an_attached_schema_are_made_again_in_it() {
    let mut table = users();
    table.structure.table = TableRef::in_schema("aux", "users");
    table.checks.clear();
    table.written_indexes = vec![SchemaObject {
        name: "users_lower".into(),
        sql: "CREATE INDEX users_lower ON users (lower(email))".into(),
    }];
    let alter = SchemaChange::AlterColumn {
        table: table.structure.table.clone(),
        column: "email".into(),
        to: ColumnDef::new("email", "VARCHAR(100)"),
    };
    let plan = rebuild(&table, &[alter]).unwrap();
    for made in [
        "CREATE INDEX \"aux\".users_lower ON users (lower(email))",
        "CREATE TRIGGER \"aux\".users_touch AFTER UPDATE ON users BEGIN SELECT NEW.email; END",
        "CREATE VIEW \"aux\".names AS SELECT name FROM users",
    ] {
        assert!(plan.statements.contains(&made.to_owned()), "{made}: {:?}", plan.statements);
    }
    assert_eq!(plan.statements[0], "DROP VIEW \"aux\".\"names\"");
}

#[test]
fn an_object_is_qualified_after_its_kind_and_if_not_exists() {
    assert_eq!(qualified("CREATE VIEW v AS SELECT 1", None).unwrap(), "CREATE VIEW v AS SELECT 1");
    assert_eq!(
        qualified("CREATE UNIQUE INDEX \"my idx\" ON t (a)", Some("a\"b")).unwrap(),
        "CREATE UNIQUE INDEX \"a\"\"b\".\"my idx\" ON t (a)"
    );
    assert_eq!(
        qualified(
            "CREATE TRIGGER IF NOT EXISTS [t r] AFTER INSERT ON t BEGIN SELECT 1; END",
            Some("s")
        )
        .unwrap(),
        "CREATE TRIGGER IF NOT EXISTS \"s\".[t r] AFTER INSERT ON t BEGIN SELECT 1; END"
    );
    assert!(qualified("CREATE TABLE t (a)", Some("s")).is_err());
    assert!(qualified("CREATE VIEW s.v AS SELECT 1", Some("s")).is_err());
    assert!(names("SELECT \"Name\" FROM t", "name"));
    assert!(!names("SELECT 'name' FROM t -- name", "name"));
}

#[test]
fn collations_and_generated_columns_survive_and_are_never_copied() {
    let mut table = users();
    table.checks.clear();
    table.views.clear();
    table.triggers.clear();
    table.structure.columns[1].collation = Some("NOCASE".into());
    let mut total = column("total", "INTEGER");
    total.generated = Some(Generated { expression: "id * 2".into(), stored: true });
    let mut shown = column("shown", "TEXT");
    shown.generated = Some(Generated { expression: "upper(name)".into(), stored: false });
    table.structure.columns.extend([total, shown]);
    let plan =
        rebuild(&table, &[SchemaChange::DropColumn { table: t(), column: "team".into() }]).unwrap();
    assert!(
        plan.statements[0].contains(
            "\"name\" TEXT COLLATE \"NOCASE\" NOT NULL DEFAULT ('x'), \"email\" TEXT, \
             \"total\" INTEGER GENERATED ALWAYS AS (id * 2) STORED, \
             \"shown\" TEXT GENERATED ALWAYS AS (upper(name)) VIRTUAL"
        ),
        "{}",
        plan.statements[0]
    );
    assert_eq!(
        plan.statements[1],
        "INSERT INTO \"new_users\" (\"id\", \"name\", \"email\") SELECT \"id\", \"name\", \"email\" FROM \"users\""
    );
    // A stored generated column can't be added by `ALTER TABLE`; a virtual one can.
    let mut add = ColumnDef::new("twice", "INTEGER");
    add.generated = Some(Generated { expression: "id * 2".into(), stored: true });
    let s = &table.structure;
    assert!(needs_rebuild(&SchemaChange::AddColumn { table: t(), column: add.clone() }, s));
    add.generated = Some(Generated { expression: "id * 2".into(), stored: false });
    assert!(!needs_rebuild(&SchemaChange::AddColumn { table: t(), column: add.clone() }, s));
    add.default = Some(DefaultValue::Value(Value::Int(1)));
    assert!(Sqlite.column_definition(&add).is_err(), "generated with a default");
}

#[test]
fn a_column_changed_only_in_its_name_is_renamed() {
    let s = users().structure;
    let same = SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("title", "text")
            .not_null()
            .default(DefaultValue::Value(Value::Text("x".into()))),
    };
    assert_eq!(
        simplified(&same, &s),
        SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "title".into() }
    );
    assert!(!needs_rebuild(&same, &s));
    let retyped = SchemaChange::AlterColumn {
        table: t(),
        column: "name".into(),
        to: ColumnDef::new("title", "VARCHAR(5)").not_null(),
    };
    assert_eq!(simplified(&retyped, &s), retyped);
    assert!(needs_rebuild(&retyped, &s));
}

#[test]
fn rows_are_written_by_the_row_id_no_column_hides_and_inserts_give_their_keys() {
    let mut s = Structure {
        table: TableRef::new("log"),
        columns: vec![column("rowid", "TEXT"), column("line", "TEXT")],
        rowid: true,
        ..Structure::default()
    };
    let mut set = ChangeSet::new(TableRef::new("log"));
    set.changes.push(Change::Delete { key: RowKey::RowId(3) });
    set.changes.push(Change::Insert { values: vec![("line".into(), Value::Text("a".into()))] });
    assert_eq!(
        Sqlite.changes_of(&set, Some(&s)).unwrap(),
        vec![
            "DELETE FROM \"log\" WHERE _rowid_ = 3",
            "INSERT INTO \"log\" (\"line\") VALUES ('a') RETURNING _rowid_",
        ]
    );
    s.columns.push(column("_ROWID_", "TEXT"));
    assert!(Sqlite.changes_of(&set, Some(&s)).unwrap()[0].ends_with("WHERE oid = 3"));
    s.columns.push(column("oid", "TEXT"));
    assert!(Sqlite.changes_of(&set, Some(&s)).is_err(), "all three hidden: no row id");
    // A primary key: given back by its columns.
    let s = users().structure;
    let mut set = ChangeSet::new(t());
    set.changes.push(Change::Insert { values: vec![] });
    assert_eq!(
        Sqlite.changes_of(&set, Some(&s)).unwrap(),
        vec!["INSERT INTO \"users\" DEFAULT VALUES RETURNING \"id\""]
    );
    assert_eq!(Sqlite.changes(&set).unwrap(), vec!["INSERT INTO \"users\" DEFAULT VALUES"]);
    // A generated column is never written.
    let mut s = s;
    s.columns[3].generated = Some(Generated { expression: "id".into(), stored: false });
    let mut set = ChangeSet::new(t());
    set.changes.push(Change::Insert { values: vec![("team".into(), Value::Int(1))] });
    assert!(Sqlite.changes_of(&set, Some(&s)).is_err());
}

#[test]
fn a_rebuild_of_a_table_without_rowid_and_in_a_schema() {
    let mut table = users();
    table.structure.table = TableRef::in_schema("aux", "users");
    table.structure.rowid = false;
    table.views.clear();
    table.triggers.clear();
    table.checks.clear();
    let plan = rebuild(
        &table,
        &[SchemaChange::DropIndex {
            table: table.structure.table.clone(),
            name: "users_name".into(),
        }],
    )
    .unwrap();
    assert!(plan.statements[0].starts_with("CREATE TABLE \"aux\".\"new_users\" ("));
    assert!(plan.statements[0].ends_with(") WITHOUT ROWID"));
    assert_eq!(plan.statements[3], "ALTER TABLE \"aux\".\"new_users\" RENAME TO \"users\"");
    assert_eq!(plan.statements.len(), 4, "the index dropped isn't made again");
}

#[test]
fn a_rebuild_refuses_what_isnt_one() {
    let other = SchemaChange::DropColumn { table: TableRef::new("teams"), column: "id".into() };
    assert!(rebuild(&users(), &[other]).is_err(), "another table");
    assert!(rebuild(&users(), &[SchemaChange::DropTable { table: t() }]).is_err());
    let missing = SchemaChange::DropColumn { table: t(), column: "nope".into() };
    assert!(matches!(rebuild(&users(), &[missing]), Err(DbError::Query(_))));
    let twice = SchemaChange::AddColumn { table: t(), column: ColumnDef::new("email", "TEXT") };
    assert!(rebuild(&users(), &[twice]).is_err());
    let all: Vec<SchemaChange> = ["id", "name", "email", "team"]
        .iter()
        .map(|c| SchemaChange::DropColumn { table: t(), column: (*c).into() })
        .collect();
    assert!(rebuild(&users(), &all).is_err(), "a table without columns");
}

#[test]
fn a_column_without_a_type_is_rebuilt_without_one() {
    let mut table = users();
    table.structure.columns.push(column("anything", ""));
    table.checks.clear();
    let drop = SchemaChange::DropColumn { table: t(), column: "team".into() };
    let plan = rebuild(&table, &[drop]).unwrap();
    let create = plan.statements.iter().find(|s| s.starts_with("CREATE TABLE")).unwrap();
    assert!(create.contains("\"email\" TEXT, \"anything\", PRIMARY KEY"), "{create}");
    assert!(Sqlite.type_name("bad; type").is_err(), "a type is still checked");
}

#[test]
fn a_new_table_has_its_constraints_and_may_have_no_row_ids() {
    let d = Sqlite;
    let mut def = TableDef::new(
        t(),
        vec![ColumnDef::new("a", "INTEGER").not_null(), ColumnDef::new("b", "TEXT")],
    );
    def.primary_key = vec!["a".into(), "b".into()];
    def.uniques = vec![vec!["b".into()]];
    def.checks = vec!["a > 0".into()];
    def.without_rowid = true;
    assert_eq!(
        d.create_table(&def).unwrap(),
        "CREATE TABLE \"users\" (\"a\" INTEGER NOT NULL, \"b\" TEXT, PRIMARY KEY (\"a\", \"b\"), \
         UNIQUE (\"b\"), CHECK (a > 0)) WITHOUT ROWID"
    );
    // Without row ids, a key is needed; a check is one expression.
    let keyless = TableDef { primary_key: Vec::new(), ..def.clone() };
    assert!(d.create_table(&keyless).is_err());
    let bad = TableDef { checks: vec!["1); DROP TABLE x; --".into()], ..def };
    assert!(d.create_table(&bad).is_err());
}

#[test]
fn an_index_has_its_columns_order_and_its_condition() {
    let index = IndexInfo {
        name: "i".into(),
        columns: vec!["a".into(), "b".into()],
        unique: true,
        descending: vec![false, true],
        predicate: Some("b IS NOT NULL".into()),
        ..IndexInfo::default()
    };
    assert_eq!(
        Sqlite.create_index(&TableRef::in_schema("aux", "t"), &index).unwrap(),
        "CREATE UNIQUE INDEX \"aux\".\"i\" ON \"t\" (\"a\", \"b\" DESC) WHERE b IS NOT NULL"
    );
    let bad = IndexInfo { predicate: Some("1; DROP TABLE t".into()), ..index };
    assert!(Sqlite.create_index(&t(), &bad).is_err());
    assert_eq!(
        index_predicate("CREATE INDEX i ON t (a) WHERE (a > 0) AND b = 'where'"),
        Some("(a > 0) AND b = 'where'".into())
    );
    assert_eq!(index_predicate("CREATE INDEX i ON t (lower(\"where\"))"), None);
}

#[test]
fn foreign_keys_and_the_columns_order_change_by_a_rebuild() {
    let table = users();
    let add = SchemaChange::AddForeignKey {
        table: t(),
        key: ForeignKey {
            name: None,
            columns: vec!["email".into()],
            table: TableRef::new("mails"),
            references: vec!["address".into()],
            on_update: FkAction::NoAction,
            on_delete: FkAction::SetNull,
        },
    };
    let drop =
        SchemaChange::DropForeignKey { table: t(), key: table.structure.foreign_keys[0].clone() };
    let moved = SchemaChange::MoveColumn { table: t(), column: "team".into(), to: 1 };
    for change in [&add, &drop, &moved] {
        assert!(needs_rebuild(change, &table.structure));
        assert!(Sqlite.schema(change).is_err(), "never plain DDL");
    }
    let plan = rebuild(&table, &[add, drop, moved]).unwrap();
    let create = plan.statements.iter().find(|s| s.starts_with("CREATE TABLE")).unwrap();
    assert!(
        create.starts_with(
            "CREATE TABLE \"new_users\" (\"id\" INTEGER NOT NULL, \"team\" INTEGER, \"name\""
        ),
        "{create}"
    );
    assert!(
        create.contains(
            "FOREIGN KEY (\"email\") REFERENCES \"mails\" (\"address\") ON DELETE SET NULL"
        )
    );
    assert!(!create.contains("REFERENCES \"teams\""), "{create}");
    let copy = plan.statements.iter().find(|s| s.starts_with("INSERT")).unwrap();
    assert!(
        copy.contains(
            "(\"id\", \"team\", \"name\", \"email\") SELECT \"id\", \"team\", \"name\", \"email\""
        ),
        "{copy}"
    );
    assert!(plan.stale.is_empty());
    // A key that isn't there.
    let missing = SchemaChange::DropForeignKey {
        table: t(),
        key: ForeignKey {
            table: TableRef::new("nowhere"),
            ..table.structure.foreign_keys[0].clone()
        },
    };
    assert!(rebuild(&table, &[missing]).is_err());
}

#[test]
fn renames_say_what_sqlite_rewrites_and_names_follow_them() {
    let table = users();
    let rename = SchemaChange::RenameColumn { table: t(), from: "name".into(), to: "full".into() };
    assert_eq!(
        rewritten(&table, &[rename], &[]),
        vec![Rewritten { kind: ObjectKind::View, name: "names".into() }]
    );
    let email = SchemaChange::RenameColumn { table: t(), from: "email".into(), to: "mail".into() };
    assert_eq!(
        rewritten(&table, std::slice::from_ref(&email), &[]),
        vec![Rewritten { kind: ObjectKind::Trigger, name: "users_touch".into() }]
    );
    // What a rebuild drops isn't rewritten.
    assert!(rewritten(&table, &[email], &[stale(ObjectKind::Trigger, "users_touch")]).is_empty());
    let table_renamed = SchemaChange::RenameTable { table: t(), to: "people".into() };
    assert_eq!(rewritten(&table, &[table_renamed], &[]).len(), 2);
    assert_eq!(
        rename_name("name = 'name' AND \"Name\" > 1", "name", "full name"),
        "\"full name\" = 'name' AND \"full name\" > 1"
    );
    assert_eq!(rename_name("age > 2", "age", "years"), "years > 2");
}
