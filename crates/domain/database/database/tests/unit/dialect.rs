use super::*;
use crate::{CellChange, CompareOp, Order};

const NASTY: &str = "x\"; DROP TABLE users; --";

fn users() -> TableRef {
    TableRef::in_schema("public", "users")
}

#[test]
fn names_are_quoted_with_their_quotes_doubled() {
    let d = GenericSql;
    assert_eq!(d.quote_identifier("users").unwrap(), "\"users\"");
    assert_eq!(d.quote_identifier(NASTY).unwrap(), "\"x\"\"; DROP TABLE users; --\"");
    assert_eq!(d.quote_identifier("a b.c").unwrap(), "\"a b.c\"", "dots and spaces stay inside");
    assert_eq!(d.quote_identifier("ăîș 名前").unwrap(), "\"ăîș 名前\"");
    assert!(d.quote_identifier("").is_err());
    assert!(d.quote_identifier("a\0b").is_err());
    assert_eq!(d.table_name(&users()).unwrap(), "\"public\".\"users\"");
    assert_eq!(d.table_name(&TableRef::new("t")).unwrap(), "\"t\"");
}

#[test]
fn values_are_literals_text_quoted_and_bytes_in_hex() {
    let d = GenericSql;
    let lit = |v: Value| d.literal(&v).unwrap();
    assert_eq!(lit(Value::Null), "NULL");
    assert_eq!(lit(Value::Bool(true)), "TRUE");
    assert_eq!(lit(Value::Int(i64::MIN)), "-9223372036854775808");
    assert_eq!(lit(Value::Float(1.5)), "1.5");
    assert_eq!(lit(Value::Float(1.0)), "1.0");
    assert_eq!(lit(Value::Float(1e-7)), "1e-7");
    assert_eq!(lit(Value::Text("O'Brien".into())), "'O''Brien'");
    assert_eq!(lit(Value::Text("'; DROP TABLE x; --".into())), "'''; DROP TABLE x; --'");
    assert_eq!(lit(Value::Text("back\\slash\nline".into())), "'back\\slash\nline'");
    assert_eq!(lit(Value::Json("{\"a\":\"it's\"}".into())), "'{\"a\":\"it''s\"}'");
    assert_eq!(lit(Value::Other("2024-01-01".into())), "'2024-01-01'");
    assert_eq!(lit(Value::Bytes(vec![0, 0xab, 0xff])), "X'00ABFF'");
    assert!(d.literal(&Value::Float(f64::NAN)).is_err());
    assert!(d.literal(&Value::Float(f64::INFINITY)).is_err());
    assert!(d.literal(&Value::Text("a\0b".into())).is_err());
}

#[test]
fn types_are_only_names_and_arguments() {
    for ok in [
        "INTEGER",
        "varchar(255)",
        "numeric(10, 2)",
        "timestamp with time zone",
        "int[]",
        "pg_catalog.text",
    ] {
        assert_eq!(check_type(ok).unwrap(), ok);
    }
    for bad in [
        "",
        "1nt",
        "text; DROP TABLE x",
        "text --",
        "varchar(10",
        "int)",
        "text'",
        "a/*b*/",
        "x\"y",
    ] {
        assert!(check_type(bad).is_err(), "{bad}");
    }
}

#[test]
fn engine_expressions_are_one_expression() {
    for ok in [
        "0",
        "'a;b'",
        "CURRENT_TIMESTAMP",
        "datetime('now', '-1 day')",
        "nextval('s'::regclass)",
        "-1",
    ] {
        assert!(check_expression(ok).is_ok(), "{ok}");
    }
    for bad in ["1; DROP TABLE x", "1 -- c", "1 /* c */", "(1", "1)", "'open", ""] {
        assert!(check_expression(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_page_is_selected_filtered_ordered_and_limited() {
    let d = GenericSql;
    let mut page = Page::first(50);
    assert_eq!(
        d.select(&users(), &[], &page).unwrap(),
        "SELECT * FROM \"public\".\"users\" LIMIT 50"
    );
    page.offset = 100;
    page.order = vec![
        Order { column: "name".into(), descending: false },
        Order { column: NASTY.into(), descending: true },
    ];
    page.filter = Some(Filter::And(vec![
        Filter::Compare { column: "age".into(), op: CompareOp::GreaterEq, value: Value::Int(18) },
        Filter::Or(vec![
            Filter::Contains { column: "name".into(), text: "50%_O'B\\".into() },
            Filter::IsNull("email".into()),
        ]),
        Filter::Not(Box::new(Filter::NotNull("deleted".into()))),
    ]));
    assert_eq!(
        d.select(&users(), &["id".into(), "name".into()], &page).unwrap(),
        "SELECT \"id\", \"name\" FROM \"public\".\"users\" WHERE (\"age\" >= 18) AND \
         ((LOWER(CAST(\"name\" AS TEXT)) LIKE '%50\\%\\_o''b\\\\%' ESCAPE '\\') OR (\"email\" IS NULL)) \
         AND (NOT (\"deleted\" IS NOT NULL)) ORDER BY \"name\" ASC, \"x\"\"; DROP TABLE users; --\" DESC \
         LIMIT 50 OFFSET 100"
    );
    assert_eq!(
        d.count(&users(), Some(&Filter::Or(vec![]))).unwrap(),
        "SELECT COUNT(*) FROM \"public\".\"users\" WHERE 1 = 0"
    );
    let null = Filter::Compare { column: "a".into(), op: CompareOp::Eq, value: Value::Null };
    assert!(d.filter(&null).is_err(), "null compares with IsNull");
}

#[test]
fn a_change_set_is_one_statement_per_change() {
    let d = GenericSql;
    let key = RowKey::Primary(vec![("id".into(), Value::Int(7))]);
    let mut set = ChangeSet::new(users());
    set.changes = vec![
        Change::Insert {
            values: vec![
                ("name".into(), Value::Text("O'Brien".into())),
                (NASTY.into(), Value::Null),
            ],
        },
        Change::Insert { values: vec![] },
        Change::Update {
            key: key.clone(),
            cells: vec![
                CellChange {
                    column: "name".into(),
                    old: Value::Text("Ann".into()),
                    new: Value::Text("Bo".into()),
                },
                CellChange {
                    column: "note".into(),
                    old: Value::Null,
                    new: Value::Text("x".into()),
                },
                // Not compared: floats aren't exact, JSON has no equality everywhere.
                CellChange {
                    column: "score".into(),
                    old: Value::Float(0.1),
                    new: Value::Float(0.2),
                },
                CellChange {
                    column: "meta".into(),
                    old: Value::Json("{}".into()),
                    new: Value::Json("[]".into()),
                },
            ],
        },
        Change::Delete {
            key: RowKey::Primary(vec![("a".into(), Value::Int(1)), ("b".into(), Value::Null)]),
        },
    ];
    assert_eq!(
        d.changes(&set).unwrap(),
        vec![
            "INSERT INTO \"public\".\"users\" (\"name\", \"x\"\"; DROP TABLE users; --\") VALUES ('O''Brien', NULL)",
            "INSERT INTO \"public\".\"users\" DEFAULT VALUES",
            "UPDATE \"public\".\"users\" SET \"name\" = 'Bo', \"note\" = 'x', \"score\" = 0.2, \"meta\" = '[]' \
             WHERE \"id\" = 7 AND \"name\" = 'Ann' AND \"note\" IS NULL",
            "DELETE FROM \"public\".\"users\" WHERE \"a\" = 1 AND \"b\" IS NULL",
        ]
    );
    // Rows the generic dialect can't find.
    for key in [RowKey::RowId(1), RowKey::Line { line: 1, fingerprint: 0 }, RowKey::Primary(vec![])]
    {
        let mut set = ChangeSet::new(users());
        set.changes.push(Change::Delete { key });
        assert!(d.changes(&set).is_err());
    }
    let mut empty = ChangeSet::new(users());
    empty.changes.push(Change::Update { key, cells: vec![] });
    assert!(d.changes(&empty).is_err());
}

#[test]
fn schema_changes_are_standard_ddl() {
    let d = GenericSql;
    let ddl = |c: SchemaChange| d.schema(&c).unwrap();
    let mut id = ColumnDef::new("id", "bigint").not_null();
    id.auto_increment = true;
    assert_eq!(
        ddl(SchemaChange::CreateTable(TableDef {
            table: users(),
            columns: vec![
                id,
                ColumnDef::new("name", "varchar(100)")
                    .default(DefaultValue::Value(Value::Text("it's".into()))),
                ColumnDef::new("created", "timestamp")
                    .not_null()
                    .default(DefaultValue::CurrentTimestamp),
                ColumnDef::new("team", "integer"),
            ],
            primary_key: vec!["id".into()],
            foreign_keys: vec![ForeignKey {
                name: Some("users_team".into()),
                columns: vec!["team".into()],
                table: TableRef::in_schema("public", "teams"),
                references: vec!["id".into()],
                on_update: FkAction::NoAction,
                on_delete: FkAction::SetNull,
            }],
            ..TableDef::default()
        })),
        vec![
            "CREATE TABLE \"public\".\"users\" (\"id\" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL, \
             \"name\" varchar(100) DEFAULT 'it''s', \"created\" timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP, \
             \"team\" integer, PRIMARY KEY (\"id\"), CONSTRAINT \"users_team\" FOREIGN KEY (\"team\") \
             REFERENCES \"public\".\"teams\" (\"id\") ON DELETE SET NULL)"
        ]
    );
    assert_eq!(
        ddl(SchemaChange::RenameTable { table: users(), to: NASTY.into() }),
        vec!["ALTER TABLE \"public\".\"users\" RENAME TO \"x\"\"; DROP TABLE users; --\""]
    );
    assert_eq!(
        ddl(SchemaChange::DropTable { table: users() }),
        vec!["DROP TABLE \"public\".\"users\""]
    );
    assert_eq!(
        ddl(SchemaChange::AddColumn { table: users(), column: ColumnDef::new("e\"mail", "text") }),
        vec!["ALTER TABLE \"public\".\"users\" ADD COLUMN \"e\"\"mail\" text"]
    );
    assert_eq!(
        ddl(SchemaChange::RenameColumn { table: users(), from: "a".into(), to: "b".into() }),
        vec!["ALTER TABLE \"public\".\"users\" RENAME COLUMN \"a\" TO \"b\""]
    );
    assert_eq!(
        ddl(SchemaChange::AlterColumn {
            table: users(),
            column: "age".into(),
            to: ColumnDef::new("years", "smallint")
                .not_null()
                .default(DefaultValue::Value(Value::Int(0))),
        }),
        vec![
            "ALTER TABLE \"public\".\"users\" ALTER COLUMN \"age\" TYPE smallint",
            "ALTER TABLE \"public\".\"users\" ALTER COLUMN \"age\" SET NOT NULL",
            "ALTER TABLE \"public\".\"users\" ALTER COLUMN \"age\" SET DEFAULT 0",
            "ALTER TABLE \"public\".\"users\" RENAME COLUMN \"age\" TO \"years\"",
        ]
    );
    assert_eq!(
        ddl(SchemaChange::DropColumn { table: users(), column: "age".into() }),
        vec!["ALTER TABLE \"public\".\"users\" DROP COLUMN \"age\""]
    );
    let index = IndexInfo {
        name: "users_name".into(),
        columns: vec!["name".into(), "age".into()],
        unique: true,
        implicit: false,
        ..IndexInfo::default()
    };
    assert_eq!(
        ddl(SchemaChange::CreateIndex { table: users(), index }),
        vec!["CREATE UNIQUE INDEX \"users_name\" ON \"public\".\"users\" (\"name\", \"age\")"]
    );
    assert_eq!(
        ddl(SchemaChange::DropIndex { table: users(), name: "users_name".into() }),
        vec!["DROP INDEX \"public\".\"users_name\""]
    );
    // What can't be written safely isn't.
    let bad_type = SchemaChange::AddColumn {
        table: users(),
        column: ColumnDef::new("a", "text); DROP TABLE x; --"),
    };
    assert!(d.schema(&bad_type).is_err());
    let bad_default = SchemaChange::AddColumn {
        table: users(),
        column: ColumnDef::new("a", "int")
            .default(DefaultValue::Expression("1; DROP TABLE x".into())),
    };
    assert!(d.schema(&bad_default).is_err());
    let no_columns = SchemaChange::CreateTable(TableDef {
        table: users(),
        columns: vec![],
        primary_key: vec![],
        foreign_keys: vec![],
        ..TableDef::default()
    });
    assert!(d.schema(&no_columns).is_err());
    assert_eq!((d.begin(), d.commit(), d.rollback()), ("BEGIN", "COMMIT", "ROLLBACK"));
}

#[test]
fn foreign_keys_are_added_and_dropped_by_name_and_a_move_is_refused() {
    let d = GenericSql;
    let key = ForeignKey {
        name: Some("users_team".into()),
        columns: vec!["team".into()],
        table: TableRef::new("teams"),
        references: vec!["id".into()],
        on_update: FkAction::Cascade,
        on_delete: FkAction::NoAction,
    };
    assert_eq!(
        d.schema(&SchemaChange::AddForeignKey { table: users(), key: key.clone() }).unwrap(),
        vec![
            "ALTER TABLE \"public\".\"users\" ADD CONSTRAINT \"users_team\" FOREIGN KEY (\"team\") \
             REFERENCES \"teams\" (\"id\") ON UPDATE CASCADE"
        ]
    );
    assert_eq!(
        d.schema(&SchemaChange::DropForeignKey { table: users(), key: key.clone() }).unwrap(),
        vec!["ALTER TABLE \"public\".\"users\" DROP CONSTRAINT \"users_team\""]
    );
    let unnamed = ForeignKey { name: None, ..key };
    assert!(d.schema(&SchemaChange::DropForeignKey { table: users(), key: unnamed }).is_err());
    let moved = SchemaChange::MoveColumn { table: users(), column: "a".into(), to: 0 };
    assert!(d.schema(&moved).is_err());
    let rowless = TableDef {
        without_rowid: true,
        ..TableDef::new(users(), vec![ColumnDef::new("a", "int")])
    };
    assert!(d.create_table(&rowless).is_err(), "row ids are SQLite's");
    let all = crate::SchemaChanges::ALL;
    assert!(all.allows(&moved));
    let csv = crate::SchemaChanges { columns: true, renames: true, ..Default::default() };
    assert!(!csv.allows(&moved) && !csv.allows(&SchemaChange::DropTable { table: users() }));
    assert!(!d.type_suggestions().is_empty());
}

/// A console's query wrapped to read its result's next page, or its rows filtered and
/// sorted by the engine: its `;` and a last line's comment never break the wrapping.
#[test]
fn a_query_is_wrapped_to_be_read_over() {
    let d = GenericSql;
    let query = "SELECT * FROM users -- the users\n;";
    assert_eq!(
        d.select_over(query, None, &[], Some(500), 1000).unwrap(),
        "SELECT * FROM (\nSELECT * FROM users -- the users\n) AS console_result LIMIT 500 OFFSET 1000"
    );
    assert_eq!(
        d.select_over("VALUES (1);;", None, &[], None, 0).unwrap(),
        "SELECT * FROM (\nVALUES (1)\n) AS console_result"
    );
    let filter = Filter::Compare {
        column: "id".into(),
        op: crate::CompareOp::Greater,
        value: Value::Int(2),
    };
    let order = [Order { column: "name".into(), descending: true }];
    assert_eq!(
        d.select_over("select id, name from users", Some(&filter), &order, Some(10), 0).unwrap(),
        "SELECT * FROM (\nselect id, name from users\n) AS console_result WHERE \"id\" > 2 \
         ORDER BY \"name\" DESC LIMIT 10"
    );
    assert!(d.select_over("SELECT 1", None, &[], None, 5).unwrap().ends_with("OFFSET 5"));
}
