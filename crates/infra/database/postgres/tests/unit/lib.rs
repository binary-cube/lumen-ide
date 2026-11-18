use super::*;
use database::Collect;

#[test]
fn values_are_typed_by_their_column() {
    assert_eq!(value("int4", Some("42")), Value::Int(42));
    assert_eq!(value("bool", Some("t")), Value::Bool(true));
    assert_eq!(value("float8", Some("1.5")), Value::Float(1.5));
    assert_eq!(value("jsonb", Some("{\"a\":1}")), Value::Json("{\"a\":1}".into()));
    assert_eq!(value("numeric", Some("9.99")), Value::Other("9.99".into()), "kept as written");
    assert_eq!(value("text", None), Value::Null);
}

/// The demo server (`EDITOR_DB_DEMO=1`): PostgreSQL 16 on 127.0.0.1:55432, `demo` /
/// `demo123`, database `shop`.
fn demo() -> Option<Box<dyn Session>> {
    std::env::var("EDITOR_DB_DEMO").ok()?;
    let target = Target {
        host: "127.0.0.1".into(),
        port: Some(55432),
        user: "demo".into(),
        database: "shop".into(),
        ..Target::default()
    };
    Some((factory().create)().connect(&target, &Secret::new("demo123")).expect("connects"))
}

#[test]
#[ignore = "needs the demo database"]
fn explores_runs_and_cancels_on_a_real_server() {
    let Some(mut session) = demo() else { return };
    assert!(session.server().starts_with("PostgreSQL 16"), "{}", session.server());
    // The explorer: schemas › tables and views › columns and indexes.
    let schemas = session.children(&[]).unwrap();
    assert!(schemas.iter().any(|n| n.name == "public" && n.kind == "schema"));
    let tables = session.children(&["public".into()]).unwrap();
    let names: Vec<_> = tables.iter().map(|n| (n.name.as_str(), n.kind)).collect();
    assert!(
        names.contains(&("orders", "table")) && names.contains(&("big_orders", "view")),
        "{names:?}"
    );
    let columns = session.children(&["public".into(), "orders".into()]).unwrap();
    assert!(
        columns
            .iter()
            .any(|n| n.kind == "column" && n.name == "customer_id" && n.detail == "integer")
    );
    assert!(columns.iter().any(|n| n.kind == "index" && n.name == "orders_customer"));
    // Typed rows; a sink with enough stops the rest.
    let mut sink = Collect::default();
    let outcome = session
        .execute(
            "SELECT id, sku, price, stock > 0 AS in_stock, meta FROM products ORDER BY id",
            &mut sink,
            &Cancel::default(),
        )
        .unwrap();
    assert_eq!(outcome.rows, 120);
    assert_eq!(sink.columns[2].type_name, "numeric");
    assert_eq!(sink.rows[0][0], Value::Int(1));
    assert_eq!(sink.rows[0][1], Value::Text("SKU-1".into()));
    assert_eq!(sink.rows[0][2], Value::Other("10.99".into()));
    assert_eq!(sink.rows[0][3], Value::Bool(true));
    assert!(matches!(&sink.rows[0][4], Value::Json(j) if j.contains("blue")));
    let mut few = Collect { limit: Some(10), ..Collect::default() };
    let outcome = session.execute("SELECT * FROM orders", &mut few, &Cancel::default()).unwrap();
    assert!(outcome.stopped && few.rows.len() == 10);
    // The session still works after stopping early.
    let mut count = Collect::default();
    session.execute("SELECT count(*) FROM customers", &mut count, &Cancel::default()).unwrap();
    assert_eq!(count.rows[0][0], Value::Int(200));
    // Changes report what they touched (inside a transaction rolled back).
    let mut none = Collect::default();
    let outcome = session
        .execute(
            "BEGIN; UPDATE products SET stock = stock WHERE id <= 5; ROLLBACK",
            &mut none,
            &Cancel::default(),
        )
        .unwrap();
    assert!(outcome.affected.is_some());
    // A long query, cancelled from another thread.
    let cancel = Cancel::default();
    let later = cancel.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        later.cancel();
    });
    let started = std::time::Instant::now();
    let result = session.execute("SELECT pg_sleep(10)", &mut Collect::default(), &cancel);
    stopper.join().unwrap();
    assert_eq!(result, Err(DbError::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "cancelled quickly: {:?}",
        started.elapsed()
    );
    // A wrong query: the server's message.
    let wrong =
        session.execute("SELECT nope FROM orders", &mut Collect::default(), &Cancel::default());
    assert!(matches!(&wrong, Err(DbError::Sql(e)) if e.message.contains("nope")), "{wrong:?}");
}

#[test]
#[ignore = "needs the demo database"]
fn a_wrong_password_is_refused() {
    if std::env::var("EDITOR_DB_DEMO").is_err() {
        return;
    }
    let target = Target {
        host: "127.0.0.1".into(),
        port: Some(55432),
        user: "demo".into(),
        database: "shop".into(),
        ..Target::default()
    };
    let refused = (factory().create)().connect(&target, &Secret::new("wrong"));
    assert!(matches!(refused, Err(DbError::Auth(_))), "{:?}", refused.err());
}

#[test]
fn bytes_come_back_from_hex() {
    assert_eq!(value("bytea", Some("\\xdead")), Value::Bytes(vec![0xde, 0xad]));
    assert_eq!(value("bytea", Some("odd")), Value::Other("odd".into()), "kept when not hex");
}

#[test]
fn its_connection_fields() {
    let spec = connection();
    for id in ["host", "port", "user", "password", "database", "sslmode", "sslrootcert"] {
        assert!(spec.field(id).is_some(), "{id}");
    }
    assert_eq!(spec.field("sslmode").map(|f| f.default.as_str()), Some("prefer"));
    assert_eq!(spec.field("port").map(|f| f.default.as_str()), Some("5432"));
    assert!(spec.has_secret());
    assert_eq!(Postgres.connection(), spec);
    assert!(Postgres.capabilities().edit_rows && !Postgres.capabilities().schema_changes.reorder);
}

/// The servers to test against: `EDITOR_TEST_POSTGRES_URL` (one URL, or several split by
/// spaces — a 9.6 and a 17, say), each `postgresql://user:password@host:port/database`.
/// Each test makes its own schema there and drops it.
fn servers() -> Vec<(Target, Secret)> {
    let Ok(urls) = std::env::var("EDITOR_TEST_POSTGRES_URL") else { return Vec::new() };
    urls.split_whitespace()
        .map(|url| {
            let mut source = database::DataSource::new("t", ID, database::Scope::Project);
            let secret = database::fields::from_url(&connection(), url, &mut source)
                .expect("a PostgreSQL URL")
                .unwrap_or_default();
            (source.target, secret)
        })
        .collect()
}

fn run(session: &mut dyn Session, sql: &str) {
    session
        .execute(sql, &mut Collect::default(), &Cancel::default())
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

#[test]
fn tables_structure_pages_and_changes_on_real_servers() {
    for (target, secret) in servers() {
        let mut session = Postgres.connect(&target, &secret).expect("connects");
        let dialect = session.dialect().expect("its version's");
        let schema = format!("editor_test_{}", std::process::id());
        run(session.as_mut(), &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"));
        run(session.as_mut(), &format!("CREATE SCHEMA {schema}"));
        let customers = TableRef { schema: Some(schema.clone()), name: "customers".into() };
        let orders = TableRef { schema: Some(schema.clone()), name: "orders".into() };
        let make = |table: &TableRef, columns: Vec<database::ColumnDef>, key: &[&str]| {
            let mut def = database::TableDef::new(table.clone(), columns);
            def.primary_key = key.iter().map(|k| (*k).to_owned()).collect();
            database::SchemaChange::CreateTable(def)
        };
        let mut id = database::ColumnDef::new("id", "integer");
        id.auto_increment = true;
        id.nullable = false;
        let created = vec![
            make(&customers, vec![id.clone(), database::ColumnDef::new("name", "text")], &["id"]),
            make(
                &orders,
                vec![
                    id,
                    database::ColumnDef::new("customer_id", "integer"),
                    database::ColumnDef::new("total", "numeric(10, 2)"),
                    database::ColumnDef::new("data", "bytea"),
                ],
                &["id"],
            ),
            database::SchemaChange::AddForeignKey {
                table: orders.clone(),
                key: ForeignKey {
                    name: Some("orders_customer".into()),
                    columns: vec!["customer_id".into()],
                    table: customers.clone(),
                    references: vec!["id".into()],
                    on_update: database::FkAction::NoAction,
                    on_delete: database::FkAction::Cascade,
                },
            },
            database::SchemaChange::CreateIndex {
                table: orders.clone(),
                index: IndexInfo {
                    name: "orders_total".into(),
                    columns: vec!["total".into()],
                    descending: vec![true],
                    predicate: Some("total > 0".into()),
                    ..IndexInfo::default()
                },
            },
        ];
        let script = database::transaction::alter_script(dialect.as_ref(), &created).unwrap();
        database::transaction::run(session.as_mut(), &script, &Cancel::default(), &mut |_, _| {})
            .expect("the structure is made");
        // Rows added in one transaction, their keys given back.
        let mut changes = Vec::new();
        for n in 1..=25 {
            changes.push(database::Change::Insert {
                values: vec![("name".into(), Value::Text(format!("c{n}")))],
            });
        }
        let set = database::ChangeSet { table: customers.clone(), changes };
        let structure = session.structure(&customers).unwrap();
        let script =
            database::transaction::apply_script(dialect.as_ref(), &set, Some(&structure)).unwrap();
        let applied = database::transaction::run(
            session.as_mut(),
            &script,
            &Cancel::default(),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(applied.inserted.len(), 25);
        run(
            session.as_mut(),
            &format!(
                "INSERT INTO {schema}.orders (customer_id, total, data) VALUES (1, 9.5, decode('dead', 'hex'))"
            ),
        );
        // Listed, with the structure read back as the version keeps it.
        let tables = session.tables(std::slice::from_ref(&schema)).unwrap();
        let names: Vec<&str> = tables.iter().map(|t| t.table.name.as_str()).collect();
        assert_eq!(names, ["customers", "orders"]);
        let s = session.structure(&orders).unwrap();
        assert_eq!(s.primary_key(), ["id"]);
        assert!(s.column("id").unwrap().auto_increment);
        assert_eq!(s.foreign_keys[0].references, ["id"]);
        assert_eq!(s.foreign_keys[0].on_delete, database::FkAction::Cascade);
        let index = s.indexes.iter().find(|i| i.name == "orders_total").unwrap();
        assert_eq!(index.descending, [true]);
        assert!(index.predicate.as_deref().unwrap_or_default().contains("total"));
        // Pages in the key's order, the next after the last key.
        let mut seen = Vec::new();
        for offset in [0, 10, 20] {
            let page = Page { offset, ..Page::first(10) };
            let mut rows = Collect::default();
            session.read(&customers, &page, &mut rows, &Cancel::default()).unwrap();
            assert_eq!(rows.keys.len(), rows.rows.len());
            seen.extend(rows.rows.into_iter().map(|r| r[1].clone()));
        }
        assert_eq!(seen.len(), 25);
        assert_eq!(seen[24], Value::Text("c25".into()));
        let mut bytes = Collect::default();
        session.read(&orders, &Page::first(5), &mut bytes, &Cancel::default()).unwrap();
        assert_eq!(bytes.rows[0][3], Value::Bytes(vec![0xde, 0xad]));
        // A changed row found by its key; one changed since read is a conflict, all rolled back.
        let key = RowKey::Primary(vec![("id".into(), Value::Int(1))]);
        let update = |old: &str, new: &str| database::ChangeSet {
            table: customers.clone(),
            changes: vec![database::Change::Update {
                key: key.clone(),
                cells: vec![database::CellChange {
                    column: "name".into(),
                    old: Value::Text(old.into()),
                    new: Value::Text(new.into()),
                }],
            }],
        };
        let script = database::transaction::apply_script(
            dialect.as_ref(),
            &update("c1", "first"),
            Some(&structure),
        )
        .unwrap();
        database::transaction::run(session.as_mut(), &script, &Cancel::default(), &mut |_, _| {})
            .unwrap();
        let script = database::transaction::apply_script(
            dialect.as_ref(),
            &update("c1", "again"),
            Some(&structure),
        )
        .unwrap();
        let stale = database::transaction::run(
            session.as_mut(),
            &script,
            &Cancel::default(),
            &mut |_, _| {},
        );
        assert!(matches!(stale, Err(DbError::Conflict(_))), "{stale:?}");
        run(session.as_mut(), &format!("DROP SCHEMA {schema} CASCADE"));
    }
}

/// The console on each `EDITOR_TEST_POSTGRES_URL` server (none set: nothing runs).
#[test]
fn the_console_on_real_servers() {
    for (target, secret) in servers() {
        let mut session = Postgres.connect(&target, &secret).expect("connects");
        let schema = format!("editor_console_{}", std::process::id());
        run(session.as_mut(), &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"));
        run(session.as_mut(), &format!("CREATE SCHEMA {schema}"));
        run(
            session.as_mut(),
            &format!(
                "CREATE TABLE {schema}.n (id integer PRIMARY KEY, label text NOT NULL DEFAULT 'x');\
                 COMMENT ON COLUMN {schema}.n.label IS 'its name';\
                 INSERT INTO {schema}.n SELECT g, 'n' || g FROM generate_series(1, 1234) g;\
                 CREATE FUNCTION {schema}.twice(a integer) RETURNS integer AS 'SELECT a * 2' LANGUAGE sql;"
            ),
        );
        // The schema by search_path, and where the session is.
        session.set_schema(&schema).unwrap();
        let place = session.current().unwrap();
        assert_eq!(place.schema.as_deref(), Some(schema.as_str()));
        assert!(session.databases().unwrap().contains(place.database.as_ref().unwrap()));
        // A query read a page at a time from a cursor (its own read-only transaction).
        let options = RunOptions { fetch_limit: 500, keep_cursor: true };
        let mut first = Collect::default();
        let outcome = session
            .run("SELECT id, label FROM n ORDER BY id", &options, &mut first, &Cancel::default())
            .unwrap();
        assert_eq!(first.rows.len(), 500);
        assert_eq!(first.columns[0].type_name, "int4");
        assert!(outcome.truncated && !session.in_transaction());
        let token = outcome.more.expect("a cursor");
        let mut pages = vec![first.rows];
        let mut next = Some(token);
        while let Some(token) = next {
            let mut page = Collect::default();
            let outcome = session.fetch(&token, 500, &mut page, &Cancel::default()).unwrap();
            pages.push(page.rows);
            next = outcome.more;
        }
        let ids: Vec<Value> = pages.concat().into_iter().map(|r| r[0].clone()).collect();
        assert_eq!(ids.len(), 1234);
        assert_eq!(ids[1233], Value::Int(1234));
        // A write past its page runs whole.
        let outcome = session
            .run(
                "UPDATE n SET label = label RETURNING id",
                &options,
                &mut Collect::default(),
                &Cancel::default(),
            )
            .unwrap();
        assert_eq!((outcome.affected, outcome.truncated, outcome.rows), (Some(1234), true, 500));
        // Notices, an error's place and code.
        let notice = session
            .run(
                "DO $$ BEGIN RAISE NOTICE 'hello %', 7; END $$",
                &options,
                &mut Collect::default(),
                &Cancel::default(),
            )
            .unwrap();
        assert!(notice.notices.iter().any(|n| n.severity == "NOTICE" && n.message == "hello 7"));
        let text = "SELECT nope FROM n";
        let wrong = session.run(text, &options, &mut Collect::default(), &Cancel::default());
        let Err(DbError::Sql(e)) = wrong else { panic!("{wrong:?}") };
        assert_eq!(e.sqlstate.as_deref(), Some("42703"));
        assert_eq!(e.position, Some(database::sql::EnginePosition::Character(8)), "{e:?}");
        // Transactions, then the plan of a write undone.
        session
            .begin(&TxOptions { isolation: Some(Isolation::Serializable), read_only: false })
            .unwrap();
        assert!(session.in_transaction());
        session
            .run("DELETE FROM n", &options, &mut Collect::default(), &Cancel::default())
            .unwrap();
        session.rollback().unwrap();
        let plan =
            session.explain("DELETE FROM n WHERE id > 10", true, &Cancel::default()).unwrap();
        assert!(plan[0].walk().iter().any(|n| n.actual_rows.is_some()));
        let mut count = Collect::default();
        session.execute("SELECT count(*) FROM n", &mut count, &Cancel::default()).unwrap();
        assert_eq!(count.rows[0][0], Value::Int(1234), "the analyzed delete was undone");
        // Profiles: every option the version takes accepted; a write profiled inside the
        // user's transaction undone to its savepoint; what can't run in one refused.
        let profiling = database::profile::ProfileOptions {
            verbose: true,
            serialize: true,
            ..database::profile::ProfileOptions::ACTUAL
        };
        let profile =
            session.profile("SELECT * FROM n WHERE id > 5", &profiling, &Cancel::default());
        let profile = profile.unwrap_or_else(|e| panic!("{e}"));
        assert!(profile.collected.actual && profile.engine.version_num.is_some());
        assert!(profile.nodes[0].actuals.rows.is_some() && profile.raw.is_some());
        let estimated = database::profile::ProfileOptions::ESTIMATED;
        assert!(session.profile("SELECT 1", &estimated, &Cancel::default()).is_ok());
        session.begin(&TxOptions::default()).unwrap();
        let undone = session.profile("DELETE FROM n", &profiling, &Cancel::default()).unwrap();
        assert!(undone.has_note(database::profile::ProfileNote::RolledBack));
        assert!(session.in_transaction(), "the user's transaction stays open");
        let mut count = Collect::default();
        session.execute("SELECT count(*) FROM n", &mut count, &Cancel::default()).unwrap();
        assert_eq!(count.rows[0][0], Value::Int(1234), "the profiled delete was undone");
        session.rollback().unwrap();
        // The table's statistics (and inside a transaction, which stays usable); the query's
        // history asked: read when pg_stat_statements is there (14+), noted missing otherwise.
        run(session.as_mut(), "ANALYZE n");
        let table = TableRef::in_schema(schema.as_str(), "n");
        let stats = session.table_stats(std::slice::from_ref(&table), &Cancel::default()).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].table, table);
        assert!(stats[0].table_bytes.is_some_and(|b| b > 0));
        assert!(stats[0].indexes.iter().any(|i| i.unique && i.columns == ["id"]));
        session.begin(&TxOptions::default()).unwrap();
        let unknown = TableRef::in_schema(schema.as_str(), "nope");
        assert!(session.table_stats(&[unknown], &Cancel::default()).unwrap().is_empty());
        let mut count = Collect::default();
        session.execute("SELECT count(*) FROM n", &mut count, &Cancel::default()).unwrap();
        assert_eq!(count.rows[0][0], Value::Int(1234), "the transaction still works");
        session.rollback().unwrap();
        let history = database::profile::ProfileOptions {
            statement_history: true,
            ..database::profile::ProfileOptions::ACTUAL
        };
        let profiled =
            session.profile("SELECT * FROM n WHERE id = 7", &history, &Cancel::default());
        let profiled = profiled.unwrap_or_else(|e| panic!("{e}"));
        assert!(profiled.nodes.iter().all(|n| n.output.is_empty()), "no columns asked");
        assert!(
            profiled.statement.history.is_some()
                || profiled.has_note(database::profile::ProfileNote::NoStatementStats)
                || profiled.statement.query_id.is_none()
                || profile_version(&profiled) < 140_000
        );
        assert!(matches!(
            session.profile("VACUUM n", &profiling, &Cancel::default()),
            Err(DbError::Unsupported(_))
        ));
        // The catalog in bulk, routines by the version's columns.
        let columns = session.catalog_columns(Some(&schema), &Cancel::default()).unwrap();
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].1[0].key_position, Some(1));
        assert_eq!(columns[0].1[1].comment.as_deref(), Some("its name"));
        let routines = session.routines(Some(&schema), &Cancel::default()).unwrap();
        assert!(routines.iter().any(|r| r.name == "twice" && r.arguments == "a integer"));
        let builtin = session.routines(None, &Cancel::default()).unwrap();
        assert!(builtin.iter().any(|r| r.name == "count" && r.kind == RoutineKind::Aggregate));
        // A long run cancelled.
        let cancel = Cancel::default();
        let later = cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            later.cancel();
        });
        let result = session.run("SELECT pg_sleep(10)", &options, &mut Collect::default(), &cancel);
        stopper.join().unwrap();
        assert_eq!(result, Err(DbError::Cancelled));
        run(session.as_mut(), &format!("DROP SCHEMA {schema} CASCADE"));
        // A read-only source's session writes nothing.
        let mut read_only = target.clone();
        read_only.options.insert(Target::READ_ONLY.into(), "true".into());
        let mut session = Postgres.connect(&read_only, &secret).expect("connects");
        let refused = session.execute(
            "CREATE TEMP TABLE t (x int)",
            &mut Collect::default(),
            &Cancel::default(),
        );
        assert!(refused.is_err(), "{refused:?}");
    }
}

/// A profile's server version (`server_version_num`).
fn profile_version(profile: &database::profile::Profile) -> u32 {
    profile.engine.version_num.unwrap_or(0)
}
