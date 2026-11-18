use super::*;
use crate::{Change, ColumnInfo, RowKey};

fn users() -> TableRef {
    TableRef::new("users")
}

fn structure() -> Structure {
    let mut name = ColumnInfo::new("name", "varchar(255)");
    name.nullable = true;
    Structure {
        table: users(),
        columns: vec![ColumnInfo::new("age", "integer"), name],
        ..Structure::default()
    }
}

#[test]
fn dropping_a_table_is_always_dangerous_a_column_only_with_data() {
    let changes = [
        SchemaChange::DropTable { table: users() },
        SchemaChange::DropColumn { table: users(), column: "age".into() },
    ];
    let empty = schema_dangers(&changes, &|_| Some(0), &|_| None);
    assert_eq!(empty, vec![Danger::DropTable { table: users(), rows: Some(0) }]);

    let full = schema_dangers(&changes, &|_| Some(12), &|_| None);
    assert_eq!(full.len(), 2);
    assert_eq!(
        full[1],
        Danger::DropColumn { table: users(), column: "age".into(), rows: Some(12) }
    );
    // Unknown rows count as some.
    let unknown = schema_dangers(&changes[1..], &|_| None, &|_| None);
    assert_eq!(
        unknown,
        vec![Danger::DropColumn { table: users(), column: "age".into(), rows: None }]
    );
    // Nothing else is.
    let safe = [
        SchemaChange::AddColumn { table: users(), column: ColumnDef::new("email", "text") },
        SchemaChange::RenameColumn { table: users(), from: "name".into(), to: "full_name".into() },
    ];
    assert!(schema_dangers(&safe, &|_| None, &|_| None).is_empty());
}

#[test]
fn changing_a_column_is_dangerous_when_it_may_change_its_values() {
    let alter = |to: ColumnDef, column: &str| {
        schema_dangers(
            &[SchemaChange::AlterColumn { table: users(), column: column.into(), to }],
            &|_| Some(5),
            &|_| Some(structure()),
        )
    };
    let loss = |d: Vec<Danger>| match d.as_slice() {
        [Danger::AlterColumn { loss, .. }] => Some(*loss),
        [] => None,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        loss(alter(ColumnDef::new("age", "text"), "age")),
        Some(AlterColumnLoss::Conversion)
    );
    assert_eq!(
        loss(alter(ColumnDef::new("name", "varchar(10)"), "name")),
        Some(AlterColumnLoss::Resize)
    );
    assert_eq!(
        loss(alter(ColumnDef::new("name", "VARCHAR(255)").not_null(), "name")),
        Some(AlterColumnLoss::NotNull)
    );
    assert_eq!(
        loss(alter(ColumnDef::new("name", "VARCHAR( 255 )"), "name")),
        Some(AlterColumnLoss::Resize)
    );
    assert_eq!(
        loss(alter(ColumnDef::new("nick", "varchar(255)"), "name")),
        None,
        "a rename keeps values"
    );
    // A column the structure doesn't know: a conversion, to be safe.
    assert_eq!(loss(alter(ColumnDef::new("x", "text"), "x")), Some(AlterColumnLoss::Conversion));
}

#[test]
fn deleting_many_rows_or_all_of_them_is_a_mass_delete() {
    let mut set = ChangeSet::new(users());
    for id in 0..3 {
        set.changes.push(Change::Delete { key: RowKey::RowId(id) });
    }
    assert!(change_dangers(&set, Some(1000)).is_empty());
    assert_eq!(
        change_dangers(&set, Some(3)),
        vec![Danger::MassDelete { table: users(), rows: 3 }],
        "every row of the table"
    );
    for id in 3..MASS_DELETE as i64 {
        set.changes.push(Change::Delete { key: RowKey::RowId(id) });
    }
    assert_eq!(change_dangers(&set, None).len(), 1);
    // A table's one row is just a delete.
    let mut one = ChangeSet::new(users());
    one.changes.push(Change::Delete { key: RowKey::RowId(1) });
    assert!(change_dangers(&one, Some(1)).is_empty());
}

#[test]
fn what_a_rebuild_drops_without_making_it_again_is_lost() {
    use crate::sqlite::{ObjectKind, Stale};
    use crate::transaction::Script;
    let mut script = Script::transaction(&crate::sqlite::Sqlite, Vec::new());
    assert!(script_dangers(&script).is_empty());
    script.stale.push(Stale {
        table: TableRef::new("users"),
        kind: ObjectKind::View,
        name: "names".into(),
    });
    assert_eq!(
        script_dangers(&script),
        vec![Danger::LostObject {
            table: TableRef::new("users"),
            kind: ObjectKind::View,
            name: "names".into()
        }]
    );
}

#[test]
fn dropping_an_index_is_confirmed_too() {
    let drop = SchemaChange::DropIndex { table: users(), name: "users_age".into() };
    assert_eq!(
        schema_dangers(&[drop], &|_| Some(0), &|_| None),
        vec![Danger::DropIndex { table: users(), name: "users_age".into() }]
    );
}

mod statements {
    use super::*;
    use crate::sql::{Flavor, WriteKind, lex};

    const PG: Flavor = Flavor::Postgres { version: None };

    fn dangers(text: &str, flavor: Flavor) -> Vec<Danger> {
        statement_dangers(text, &lex(text, flavor), flavor)
    }

    fn unfiltered(kind: WriteKind, table: TableRef) -> Danger {
        Danger::UnfilteredWrite { kind, table }
    }

    #[test]
    fn updates_and_deletes_without_where_touch_every_row() {
        assert_eq!(dangers("DELETE FROM users", PG), [unfiltered(WriteKind::Delete, users())]);
        assert_eq!(dangers("update users set a = 1", PG), [unfiltered(WriteKind::Update, users())]);
        assert_eq!(
            dangers("UPDATE ONLY public.users u SET a = 1 RETURNING *", PG),
            [unfiltered(WriteKind::Update, TableRef::in_schema("public", "users"))]
        );
        assert_eq!(
            dangers("delete from only \"My Table\"", PG),
            [unfiltered(WriteKind::Delete, TableRef::new("My Table"))]
        );
        assert_eq!(
            dangers("UPDATE OR IGNORE [users] SET a = 1", Flavor::Sqlite),
            [unfiltered(WriteKind::Update, users())]
        );
        assert_eq!(
            dangers("DELETE LOW_PRIORITY FROM `users` LIMIT 10", Flavor::MySql),
            [unfiltered(WriteKind::Delete, users())],
            "a LIMIT isn't a filter"
        );
        // Joined without a condition: still every row.
        assert_eq!(
            dangers("DELETE FROM users USING orders", PG),
            [unfiltered(WriteKind::Delete, users())]
        );
    }

    #[test]
    fn a_where_only_counts_at_the_statements_own_level() {
        assert_eq!(
            dangers("DELETE FROM users WHERE id IN (SELECT id FROM banned WHERE x)", PG),
            []
        );
        assert_eq!(
            dangers("UPDATE users SET a = (SELECT b FROM t WHERE t.id = 1)", PG),
            [unfiltered(WriteKind::Update, users())],
            "the subquery's WHERE filters the subquery"
        );
        assert_eq!(dangers("DELETE FROM users USING orders o WHERE o.user_id = users.id", PG), []);
        assert_eq!(dangers("UPDATE users SET a = 1 WHERE CURRENT OF c", PG), []);
        assert_eq!(dangers("delete from users where id = 1 returning *", PG), []);
        assert_eq!(dangers("UPDATE users SET a = 'WHERE'", PG).len(), 1, "in a string: text");
        assert_eq!(dangers("UPDATE users SET a = 1 -- WHERE id = 1", PG).len(), 1, "in a comment");
    }

    #[test]
    fn a_where_that_is_always_true_is_no_filter() {
        for condition in [
            "true",
            "TRUE",
            "1",
            "1=1",
            "1 = 1",
            "'a' = 'a'",
            "(1 = 1)",
            "((true))",
            "not false",
            "id = 5 OR 1 = 1",
            "1 = 1 or id = 5",
            "",
        ] {
            let text = format!("DELETE FROM users WHERE {condition}");
            assert_eq!(dangers(&text, PG).len(), 1, "{text}");
        }
        for condition in [
            "id = 1",
            "1 = 1 AND id = 5",
            "0 = 1",
            "1 = 2",
            "a = a",
            "false",
            "0",
            "(id = 1) or (id = 2)",
        ] {
            let text = format!("UPDATE users SET a = 1 WHERE {condition} RETURNING id");
            assert!(dangers(&text, PG).is_empty(), "{text}");
        }
    }

    #[test]
    fn writes_in_a_with_and_under_explain_analyze() {
        assert_eq!(
            dangers("WITH gone AS (DELETE FROM users RETURNING *) SELECT count(*) FROM gone", PG),
            [unfiltered(WriteKind::Delete, users())]
        );
        assert_eq!(
            dangers("WITH ids AS (SELECT id FROM t WHERE x) UPDATE users SET a = 1", PG),
            [unfiltered(WriteKind::Update, users())],
            "a part's WHERE isn't the main statement's"
        );
        assert_eq!(
            dangers(
                "WITH ids AS (SELECT id FROM t) DELETE FROM users WHERE id IN (SELECT id FROM ids)",
                PG
            ),
            []
        );
        assert_eq!(dangers("EXPLAIN ANALYZE DELETE FROM users", PG).len(), 1, "it runs");
        assert_eq!(dangers("EXPLAIN (ANALYZE, BUFFERS) DELETE FROM users", PG).len(), 1);
        assert!(dangers("EXPLAIN DELETE FROM users", PG).is_empty(), "only planned");
        assert!(dangers("EXPLAIN QUERY PLAN DELETE FROM users", Flavor::Sqlite).is_empty());
        assert!(dangers("EXPLAIN (FORMAT JSON) DELETE FROM users", PG).is_empty());
    }

    #[test]
    fn what_mentions_delete_or_update_without_doing_it() {
        for text in [
            "SELECT * FROM users FOR UPDATE",
            "INSERT INTO t VALUES (1) ON CONFLICT (id) DO UPDATE SET a = 2",
            "INSERT INTO t VALUES (1) ON DUPLICATE KEY UPDATE a = 2",
            "CREATE TABLE t (a int REFERENCES u ON DELETE CASCADE ON UPDATE SET NULL)",
            "CREATE TRIGGER t AFTER INSERT ON x BEGIN DELETE FROM log; END",
            "GRANT DELETE, UPDATE ON users TO r",
            "select 'delete from users'",
        ] {
            assert!(dangers(text, Flavor::Sqlite).is_empty(), "{text}");
        }
    }

    #[test]
    fn drops_name_what_goes_and_whether_it_cascades() {
        let drop =
            |kind, name: &str, cascade| Danger::DropObject { kind, name: name.into(), cascade };
        assert_eq!(dangers("DROP TABLE users", PG), [drop(DropKind::Table, "users", false)]);
        assert_eq!(
            dangers("drop table if exists public.a, b cascade", PG),
            [drop(DropKind::Table, "public.a", true), drop(DropKind::Table, "b", true)]
        );
        assert_eq!(dangers("DROP MATERIALIZED VIEW v", PG), [drop(DropKind::View, "v", false)]);
        assert_eq!(dangers("DROP VIEW data", PG), [drop(DropKind::View, "data", false)]);
        assert_eq!(
            dangers("DROP INDEX CONCURRENTLY IF EXISTS i", PG),
            [drop(DropKind::Index, "i", false)]
        );
        assert_eq!(
            dangers("DROP INDEX i ON t", Flavor::MySql),
            [drop(DropKind::Index, "i", false)]
        );
        assert_eq!(dangers("DROP SCHEMA s CASCADE", PG), [drop(DropKind::Schema, "s", true)]);
        assert_eq!(dangers("DROP DATABASE shop", PG), [drop(DropKind::Database, "shop", false)]);
        assert_eq!(
            dangers("DROP FUNCTION f(int, text), g()", PG),
            [drop(DropKind::Function, "f", false), drop(DropKind::Function, "g", false)]
        );
        assert_eq!(dangers("DROP PROCEDURE p", PG), [drop(DropKind::Procedure, "p", false)]);
        assert_eq!(dangers("DROP TRIGGER t ON x", PG), [drop(DropKind::Trigger, "t", false)]);
        assert_eq!(dangers("DROP SEQUENCE s", PG), [drop(DropKind::Sequence, "s", false)]);
        assert_eq!(dangers("DROP TYPE mood", PG), [drop(DropKind::Type, "mood", false)]);
        assert_eq!(dangers("DROP DOMAIN d", PG), [drop(DropKind::Type, "d", false)]);
        assert_eq!(dangers("DROP FOREIGN TABLE f", PG), [drop(DropKind::Table, "f", false)]);
        assert_eq!(
            dangers("DROP TEMPORARY TABLE t", Flavor::MySql),
            [drop(DropKind::Table, "t", false)]
        );
        assert_eq!(dangers("DROP EXTENSION hstore", PG), [drop(DropKind::Other, "hstore", false)]);
        assert_eq!(dangers("DROP EVENT TRIGGER et", PG), [drop(DropKind::Other, "et", false)]);
        assert_eq!(dangers("DROP OWNED BY bob CASCADE", PG), [drop(DropKind::Other, "bob", true)]);
        assert_eq!(
            dangers("DROP", PG),
            [drop(DropKind::Other, "", false)],
            "being typed: still a drop"
        );
    }

    #[test]
    fn truncates_every_table_named() {
        let t = |name: &str| Danger::Truncate { table: TableRef::new(name) };
        assert_eq!(dangers("TRUNCATE users", PG), [t("users")]);
        assert_eq!(
            dangers("truncate table only a, b * restart identity cascade", PG),
            [t("a"), t("b")]
        );
    }

    #[test]
    fn dropped_columns_and_not_what_else_alter_table_drops() {
        let col = |c: &str| Danger::AlterDrop { table: users(), column: c.into() };
        assert_eq!(dangers("ALTER TABLE users DROP COLUMN age", PG), [col("age")]);
        assert_eq!(dangers("alter table users drop age", PG), [col("age")], "COLUMN is optional");
        assert_eq!(
            dangers(
                "ALTER TABLE IF EXISTS ONLY users DROP COLUMN IF EXISTS \"Age\" CASCADE, ADD b int, DROP c",
                PG
            ),
            [col("Age"), col("c")]
        );
        assert_eq!(dangers("ALTER TABLE users DROP COLUMN age", Flavor::Sqlite), [col("age")]);
        for text in [
            "ALTER TABLE users DROP CONSTRAINT users_pkey",
            "ALTER TABLE users ALTER COLUMN age DROP DEFAULT",
            "ALTER TABLE users ALTER COLUMN age DROP NOT NULL",
            "ALTER TABLE users DROP PRIMARY KEY",
            "ALTER TABLE users DROP INDEX i",
            "ALTER TABLE users DROP FOREIGN KEY fk",
            "ALTER TABLE users ADD COLUMN drop_me int",
            "ALTER INDEX i RENAME TO j",
        ] {
            assert!(dangers(text, PG).is_empty(), "{text}");
        }
    }

    #[test]
    fn safe_statements_have_no_dangers() {
        for text in [
            "SELECT * FROM users",
            "INSERT INTO users VALUES (1)",
            "CREATE TABLE t (a int)",
            "ALTER TABLE t ADD COLUMN b int",
            "BEGIN",
            "",
        ] {
            assert!(dangers(text, PG).is_empty(), "{text}");
        }
    }
}
