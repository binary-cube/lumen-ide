//! The driver against every server of the local test matrix (docker/databases, seeded the
//! same everywhere): `EDITOR_TEST_MYSQL_URLS`, comma-separated
//! `mysql://editor:…@host:port/shop` URLs (`scripts/test-databases.sh urls` prints them):
//! MySQL 5.7, 8.0, 8.4 and MariaDB 10.11, 11.4, 11.8. None set: nothing runs. Read-only:
//! the seed's data is only read (what writes goes in a transaction rolled back, or is
//! refused).
//!
//! What a user's source sees: connected to `shop` while the data lives in its databases
//! (`shop`, `analytics`: MySQL's schemas), a view of another user's (`DEFINER`) over a
//! table it may not read, a database it has no rights on (`editor_private`: not listed).

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use database::completion::{CompletionRequest, LIMIT, SuggestionKind, complete};
use database::mysql::ServerVersion;
use database::profile::{OpKind, ProfileDepth, ProfileOptions};
use database::schema_index::{IndexParts, SchemaIndex};
use database::sql::{EnginePosition, Flavor, lex, scope_in};
use database::{
    Cancel, Collect, DataSource, DataSourceId, DbError, Driver, Isolation, Page, RunOptions, Scope,
    Secret, Session, TableKind, TableRef, Target, TxOptions, Value,
};
use db_mysql::{ID, MySqlDriver, connection};

/// A server of the matrix: its URL's target and password.
struct Server {
    url: String,
    target: Target,
    secret: Secret,
}

fn servers() -> Vec<Server> {
    let Ok(urls) = std::env::var("EDITOR_TEST_MYSQL_URLS") else { return Vec::new() };
    urls.split(',')
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|url| {
            let mut source = DataSource::new("matrix", ID, Scope::Project);
            let secret = database::fields::from_url(&connection(), url, &mut source)
                .expect("a MySQL URL")
                .unwrap_or_default();
            Server { url: url.to_owned(), target: source.target, secret }
        })
        .collect()
}

impl Server {
    /// A session on `database` of this server (as `user`, when given: the seed's password).
    fn open(&self, database: &str, user: Option<&str>) -> Result<Box<dyn Session>, DbError> {
        let mut target = self.target.clone();
        target.database = database.to_owned();
        if let Some(user) = user {
            target.user = user.to_owned();
        }
        MySqlDriver.connect(&target, &self.secret)
    }

    fn session(&self, database: &str) -> Box<dyn Session> {
        self.open(database, None).unwrap_or_else(|e| panic!("{}: {database}: {e}", self.url))
    }
}

fn t(schema: &str, name: &str) -> TableRef {
    TableRef { schema: Some(schema.to_owned()), name: name.to_owned() }
}

/// The server's engine and version.
fn version(session: &mut dyn Session) -> ServerVersion {
    let text = rows(session, "SELECT VERSION()")[0][0].display();
    ServerVersion::parse(&text).unwrap()
}

/// `test` run on every server, each one's failure caught: all of them said at the end (one
/// line per server), so a version's failure doesn't hide another's.
fn each(test: impl Fn(&Server)) {
    let mut failed = Vec::new();
    for server in servers() {
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(&server)));
        match ran {
            Ok(()) => eprintln!("ok: {}", server.url),
            Err(e) => {
                let why = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default();
                failed.push(format!("{}: {why}", server.url));
            }
        }
    }
    assert!(failed.is_empty(), "failed on {} server(s):\n{}", failed.len(), failed.join("\n"));
}

fn rows(session: &mut dyn Session, sql: &str) -> Vec<Vec<Value>> {
    let mut out = Collect::default();
    session.execute(sql, &mut out, &Cancel::default()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    out.rows
}

#[test]
fn every_database_the_user_may_reach_is_a_schema_of_one_session() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        assert!(session.server().starts_with(v.engine()), "{}", session.server());
        let schemas: Vec<String> =
            session.children(&[]).unwrap().into_iter().map(|n| n.name).collect();
        // Not the server's own, nor one the user has no right on.
        assert_eq!(schemas, ["analytics", "shop"], "{}", server.url);
        assert_eq!(session.databases().unwrap(), ["analytics", "shop"]);
        let place = session.current().unwrap();
        assert_eq!(place.schema.as_deref(), Some("shop"), "{}", server.url);
        // A source naming no database sees them all the same.
        let mut bare = server.session("");
        let tables = bare.tables(&[]).unwrap();
        assert!(tables.iter().any(|i| i.table == t("analytics", "visits")), "{}", server.url);
        assert_eq!(bare.current().unwrap().schema, None);
        // A database it may not open is refused as a sign-in is.
        let refused = server.open("editor_private", None).map(|_| ());
        assert!(matches!(refused, Err(DbError::Auth(_))), "{}: {refused:?}", server.url);
        let wrong = MySqlDriver.connect(&server.target, &Secret::new("wrong")).map(|_| ());
        assert!(matches!(wrong, Err(DbError::Auth(_))), "{}: {wrong:?}", server.url);
    });
}

#[test]
fn every_table_and_view_is_listed_by_schema() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let tables = session.tables(&[]).unwrap();
        let kind = |table: &TableRef| tables.iter().find(|i| i.table == *table).map(|i| i.kind);
        for table in [
            t("shop", "countries"),
            t("shop", "customers"),
            t("shop", "products"),
            t("shop", "orders"),
            t("shop", "order_lines"),
            t("shop", "shipments"),
            t("analytics", "visits"),
        ] {
            assert_eq!(kind(&table), Some(TableKind::Table), "{}: {table}", server.url);
        }
        for view in
            [t("shop", "order_totals"), t("shop", "private_summary"), t("analytics", "daily")]
        {
            assert_eq!(kind(&view), Some(TableKind::View), "{}: {view}", server.url);
        }
        assert_eq!(kind(&t("shop", "coupons")).is_some(), !v.mariadb && v.number >= 80000);
        assert!(tables.iter().all(|i| i.table.schema.as_deref() != Some("mysql")));
        // The engine's estimate (InnoDB's: near the real count, never for a view).
        let customers = tables.iter().find(|i| i.table == t("shop", "customers")).unwrap();
        assert!(customers.rows.is_some(), "{}", server.url);
        assert!(
            tables.iter().find(|i| i.table == t("shop", "order_totals")).unwrap().rows.is_none()
        );
        // One schema's.
        let analytics = session.tables(&["analytics".to_owned()]).unwrap();
        let names: Vec<&str> = analytics.iter().map(|i| i.table.name.as_str()).collect();
        assert_eq!(names, ["daily", "visits"], "{}", server.url);
        // The explorer's nodes say the same.
        let views: Vec<String> = session
            .children(&["shop".to_owned()])
            .unwrap()
            .into_iter()
            .filter(|n| n.kind == "view")
            .map(|n| n.name)
            .collect();
        assert_eq!(views, ["numbers", "order_totals", "private_summary"], "{}", server.url);
        let columns: Vec<String> = session
            .children(&["shop".to_owned(), "countries".to_owned()])
            .unwrap()
            .into_iter()
            .filter(|n| n.kind == "column")
            .map(|n| n.name)
            .collect();
        assert_eq!(columns, ["code", "name"], "{}", server.url);
    });
}

#[test]
fn structures_keep_their_keys_indexes_and_generated_columns() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let lines = session.structure(&t("shop", "order_lines")).unwrap();
        assert_eq!(lines.primary_key(), ["order_id", "line_no"], "{}", server.url);
        let targets: Vec<String> = lines.foreign_keys.iter().map(|k| k.table.to_string()).collect();
        assert!(targets.contains(&"shop.orders".to_owned()), "{targets:?}");
        assert!(targets.contains(&"shop.products".to_owned()), "{targets:?}");
        let amount = lines.columns.iter().find(|c| c.name == "amount").unwrap();
        let generated = amount.generated.as_ref().expect("generated");
        assert!(generated.stored && generated.expression.contains("quantity"), "{generated:?}");
        // A composite foreign key, its columns in order, its rule.
        let shipments = session.structure(&t("shop", "shipments")).unwrap();
        let key = &shipments.foreign_keys[0];
        assert_eq!(key.columns, ["order_id", "line_no"]);
        assert_eq!(key.references, ["order_id", "line_no"]);
        assert_eq!(key.table, t("shop", "order_lines"));
        assert_eq!(key.on_delete, database::FkAction::Cascade);
        // Unique and plain indexes; the functional and descending ones of MySQL 8.
        let customers = session.structure(&t("shop", "customers")).unwrap();
        let index = |name: &str| customers.indexes.iter().find(|i| i.name == name);
        let email = index("customers_email_key").expect("the unique key's index");
        assert!(email.unique && !email.implicit);
        assert!(index("PRIMARY").is_some_and(|p| p.implicit && p.unique));
        assert!(customers.columns.iter().any(|c| c.name == "id" && c.auto_increment));
        let email_column = customers.columns.iter().find(|c| c.name == "email").unwrap();
        assert!(!email_column.nullable);
        let vip = customers.columns.iter().find(|c| c.name == "vip").unwrap();
        assert!(vip.default.as_deref().is_some_and(|d| d == "0" || d == "false"), "{vip:?}");
        let created = customers.columns.iter().find(|c| c.name == "created_at").unwrap();
        assert!(
            created
                .default
                .as_deref()
                .is_some_and(|d| d.to_lowercase().starts_with("current_timestamp")),
            "{created:?}"
        );
        let products = session.structure(&t("shop", "products")).unwrap();
        let size = products.columns.iter().find(|c| c.name == "size").unwrap();
        assert_eq!(size.default.as_deref(), Some("'M'"), "{}", server.url);
        if !v.mariadb && v.number >= 80013 {
            let lower = index("customers_lower_name").expect("the functional index");
            assert!(lower.columns[0].to_lowercase().contains("lower"), "{lower:?}");
            let orders = session.structure(&t("shop", "orders")).unwrap();
            let recent = orders.indexes.iter().find(|i| i.name == "orders_recent").unwrap();
            assert_eq!(recent.descending, [true]);
        }
        // A view over a table the user may not read: its structure all the same.
        let summary = session.structure(&t("shop", "private_summary")).unwrap();
        assert_eq!(summary.columns[0].name, "hidden");
    });
}

#[test]
fn the_grid_reads_pages_by_key_and_refuses_what_it_may_not_read() {
    each(|server| {
        let mut session = server.session("shop");
        let customers = t("shop", "customers");
        let mut first = Collect::default();
        let outcome =
            session.read(&customers, &Page::first(500), &mut first, &Cancel::default()).unwrap();
        assert_eq!((outcome.rows, first.rows.len(), first.keys.len()), (500, 500, 500));
        let mut second = Collect::default();
        let page = Page { offset: 500, ..Page::first(500) };
        session.read(&customers, &page, &mut second, &Cancel::default()).unwrap();
        assert_eq!(second.rows[0][0], Value::Int(501), "{}", server.url);
        // A composite key's pages.
        let mut lines = Collect::default();
        let page = Page { offset: 7990, ..Page::first(100) };
        session.read(&t("shop", "order_lines"), &page, &mut lines, &Cancel::default()).unwrap();
        assert_eq!(lines.rows.len(), 10);
        // Another user's view, readable; a table not.
        let mut summary = Collect::default();
        session
            .read(&t("shop", "private_summary"), &Page::first(10), &mut summary, &Cancel::default())
            .unwrap();
        assert_eq!(summary.rows, [[Value::Int(1)]], "{}", server.url);
        let refused = session.execute(
            "SELECT * FROM editor_private.hidden",
            &mut Collect::default(),
            &Cancel::default(),
        );
        let refused = refused.expect_err("not readable");
        assert_eq!(refused.sql_error().and_then(|e| e.sqlstate.as_deref()), Some("42000"));
        // An enum, json, a decimal, a date; text and numbers typed.
        let mut orders = Collect::default();
        session
            .read(&t("shop", "orders"), &Page::first(3), &mut orders, &Cancel::default())
            .unwrap();
        // (Each server numbered the seed's rows in its own order: only their kinds compare.)
        let statuses = ["new", "paid", "shipped", "cancelled"];
        assert!(
            matches!(&orders.rows[0][2], Value::Other(s) if statuses.contains(&s.as_str())),
            "{:?}",
            orders.rows
        );
        assert_eq!(orders.rows[0][0], Value::Int(1));
        let mut people = Collect::default();
        session.read(&customers, &Page::first(1), &mut people, &Cancel::default()).unwrap();
        assert!(matches!(&people.rows[0][1], Value::Text(e) if e.ends_with("@example.com")));
        assert!(matches!(&people.rows[0][5], Value::Json(_) | Value::Text(_)), "{:?}", people.rows);
        let price = rows(session.as_mut(), "SELECT price FROM products WHERE sku = 'SKU-0001'");
        assert_eq!(price[0][0], Value::Other("3.99".into()), "{}", server.url);
        // A filter (case-insensitive, as text) and a sort, by the engine.
        let filter =
            database::Filter::Contains { column: "email".into(), text: "CUSTOMER19@".into() };
        let page = Page { filter: Some(filter), ..Page::first(10) };
        let mut found = Collect::default();
        session.read(&customers, &page, &mut found, &Cancel::default()).unwrap();
        assert_eq!(found.rows.len(), 1, "{}", server.url);
    });
}

#[test]
fn the_console_runs_pages_errors_warnings_and_transactions() {
    each(|server| {
        let mut session = server.session("shop");
        let cancel = Cancel::default();
        // The first page; the rest of a query is run again (no cursor).
        let options = RunOptions { fetch_limit: 3000, keep_cursor: true };
        let mut first = Collect::default();
        let outcome = session
            .run("SELECT * FROM order_lines ORDER BY 1, 2", &options, &mut first, &cancel)
            .unwrap();
        assert!(outcome.truncated && first.rows.len() == 3000 && outcome.more.is_none());
        // A huge query stopped past its page, quickly: the server stops it.
        let started = Instant::now();
        let options = RunOptions { fetch_limit: 10, keep_cursor: false };
        let mut few = Collect::default();
        let outcome = session
            .run(
                "SELECT * FROM order_lines a, order_lines b, order_lines c",
                &options,
                &mut few,
                &cancel,
            )
            .unwrap();
        assert!(outcome.truncated && few.rows.len() == 10, "{}", server.url);
        assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
        // The same session goes on.
        let count = rows(session.as_mut(), "SELECT count(*) FROM customers");
        assert_eq!(count[0][0], Value::Int(2000));
        // An error with its place, code and state; a syntax error placed by its words.
        let refused = session
            .run(
                "SELECT nope FROM customers",
                &RunOptions::default(),
                &mut Collect::default(),
                &cancel,
            )
            .unwrap_err();
        let sql = refused.sql_error().unwrap();
        assert_eq!(sql.sqlstate.as_deref(), Some("42S22"), "{refused:?}");
        let text = "SELECT *\nFROMM customers";
        let refused = session
            .run(text, &RunOptions::default(), &mut Collect::default(), &cancel)
            .unwrap_err();
        let Some(EnginePosition::Character(at)) = refused.sql_error().and_then(|e| e.position)
        else {
            panic!("{}: {refused:?}", server.url)
        };
        assert_eq!(text.chars().skip(at - 1).take(5).collect::<String>(), "FROMM");
        // Warnings come with the outcome.
        let warned = session
            .run(
                "SELECT CAST('12x' AS SIGNED)",
                &RunOptions::default(),
                &mut Collect::default(),
                &cancel,
            )
            .unwrap();
        assert!(!warned.notices.is_empty(), "{}", server.url);
        // The schema switched; where the session is.
        session.set_schema("analytics").unwrap();
        assert_eq!(session.current().unwrap().schema.as_deref(), Some("analytics"));
        assert_eq!(rows(session.as_mut(), "SELECT count(*) FROM visits")[0][0], Value::Int(3000));
        session.set_schema("shop").unwrap();
        // Transactions, as the server says them: begun, a write undone; a DDL commits.
        session
            .begin(&TxOptions { isolation: Some(Isolation::Serializable), read_only: false })
            .unwrap();
        assert!(session.in_transaction());
        session
            .run(
                "UPDATE countries SET name = 'X' WHERE code = 'RO'",
                &RunOptions::default(),
                &mut Collect::default(),
                &cancel,
            )
            .unwrap();
        assert!(session.in_transaction());
        session.rollback().unwrap();
        assert!(!session.in_transaction());
        assert_eq!(
            rows(session.as_mut(), "SELECT name FROM countries WHERE code = 'RO'")[0][0],
            Value::Text("Romania".into())
        );
        session.run("BEGIN", &RunOptions::default(), &mut Collect::default(), &cancel).unwrap();
        assert!(session.in_transaction(), "a BEGIN written by hand");
        session
            .run(
                "ANALYZE TABLE countries",
                &RunOptions::default(),
                &mut Collect::default(),
                &cancel,
            )
            .unwrap();
        assert!(!session.in_transaction(), "{}: an implicit commit", server.url);
        // A read-only transaction refuses a write.
        session.begin(&TxOptions { isolation: None, read_only: true }).unwrap();
        let refused =
            session.execute("UPDATE countries SET name = name", &mut Collect::default(), &cancel);
        assert!(refused.is_err(), "{}", server.url);
        session.rollback().unwrap();
    });
}

#[test]
fn a_running_statement_is_cancelled_from_another_connection() {
    each(|server| {
        let mut session = server.session("shop");
        let cancel = Cancel::default();
        let stop = cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stop.cancel();
        });
        let started = Instant::now();
        let result = session.execute("SELECT SLEEP(20)", &mut Collect::default(), &cancel);
        stopper.join().unwrap();
        assert_eq!(result.unwrap_err(), DbError::Cancelled, "{}", server.url);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        // The session goes on.
        assert_eq!(rows(session.as_mut(), "SELECT 1")[0][0], Value::Int(1));
    });
}

#[test]
fn plans_and_profiles_as_each_version_can() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let cancel = Cancel::default();
        let caps = session.console().unwrap();
        let query = "SELECT c.name, count(*) FROM customers c \
                     JOIN orders o ON o.customer_id = c.id WHERE c.vip = 0 GROUP BY c.name";
        let plan = session.explain(query, false, &cancel).unwrap();
        assert!(!plan.is_empty(), "{}", server.url);
        let estimated = session.profile(query, &ProfileOptions::ESTIMATED, &cancel).unwrap();
        assert!(estimated.nodes.iter().any(|n| n.op.is_scan()), "{}: {estimated:?}", server.url);
        assert!(
            estimated
                .nodes
                .iter()
                .any(|n| n.object.as_ref().is_some_and(|o| o.name == "c" || o.name == "customers")),
            "{}",
            server.url
        );
        assert_eq!(estimated.engine.version_num, Some(v.number));
        let actual = ProfileOptions::of(ProfileDepth::Actual { timing: true });
        if !caps.explain.analyze {
            // MySQL 5.7: no plan as it ran.
            assert!(!v.mariadb && v.number < 80018);
            assert!(matches!(
                session.profile(query, &actual, &cancel),
                Err(DbError::Unsupported(_))
            ));
            return;
        }
        let ran = session.profile(query, &actual, &cancel).unwrap();
        assert!(ran.nodes.iter().any(|n| n.actuals.rows.is_some()), "{}: {ran:?}", server.url);
        assert!(ran.statement.execution_ms.is_some(), "{}", server.url);
        assert!(ran.collected.actual && ran.collected.engine_counters, "{}", server.url);
        assert!(
            ran.statement
                .counters
                .is_some_and(|c| c.rows_examined.unwrap_or(0) > 0 || c.select_scan.is_some())
        );
        assert!(!session.in_transaction());
        let plan = session.explain(query, true, &cancel).unwrap();
        assert!(plan.iter().flat_map(|n| n.walk()).any(|n| n.actual_rows.is_some()));
        // A write profiled as it runs is undone.
        let write = "UPDATE products SET price = price + 1 WHERE id < 50";
        let prices = "SELECT sum(price) FROM products";
        let before = rows(session.as_mut(), prices);
        let undone = session
            .profile(write, &ProfileOptions::of(ProfileDepth::Actual { timing: false }), &cancel)
            .unwrap();
        assert!(undone.has_note(database::profile::ProfileNote::RolledBack));
        assert_eq!(rows(session.as_mut(), prices), before, "{}", server.url);
        // Inside the user's transaction: under a savepoint, the transaction kept.
        session.begin(&TxOptions::default()).unwrap();
        session.profile(write, &actual, &cancel).unwrap();
        assert!(session.in_transaction());
        session.rollback().unwrap();
        assert_eq!(rows(session.as_mut(), prices), before, "{}", server.url);
        // What can't be undone isn't run.
        let ddl = session.profile("ALTER TABLE countries ADD COLUMN x int", &actual, &cancel);
        assert!(matches!(ddl, Err(DbError::Unsupported(_))), "{}: {ddl:?}", server.url);
        let _ = OpKind::SeqScan;
    });
}

#[test]
fn the_tables_statistics_come_from_the_catalog() {
    each(|server| {
        let mut session = server.session("shop");
        let cancel = Cancel::default();
        let stats =
            session.table_stats(&[t("shop", "customers"), t("shop", "orders")], &cancel).unwrap();
        assert_eq!(stats.len(), 2, "{}", server.url);
        let customers = stats.iter().find(|s| s.table == t("shop", "customers")).unwrap();
        assert!(customers.live_rows.is_some_and(|r| r > 0), "{}: {customers:?}", server.url);
        assert!(customers.total_bytes.is_some_and(|b| b > 0));
        let names: Vec<&str> = customers.indexes.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"PRIMARY") && names.contains(&"customers_email_key"), "{names:?}");
        let email = customers.indexes.iter().find(|i| i.name == "customers_email_key").unwrap();
        assert!(email.unique && email.columns == ["email"]);
    });
}

#[test]
fn the_catalog_in_bulk_builds_a_schema_index_that_completes() {
    each(|server| {
        let mut session = server.session("shop");
        let cancel = Cancel::default();
        let place = session.current().unwrap();
        let tables = session.tables(&[]).unwrap();
        let columns = session.catalog_columns(None, &cancel).unwrap();
        let customers = columns.iter().find(|(t, _)| *t == self::t("shop", "customers"));
        let (_, customer_columns) = customers.expect("the customers' columns");
        let email = customer_columns.iter().find(|c| c.name == "email").unwrap();
        assert_eq!(email.comment.as_deref(), Some("Where we write to them"));
        assert_eq!(customer_columns[0].key_position, Some(1));
        assert!(columns.iter().any(|(t, _)| *t == self::t("analytics", "visits")));
        let keys = session.catalog_keys(None, &cancel).unwrap();
        let shipments = keys.iter().find(|(t, _)| *t == self::t("shop", "shipments")).unwrap();
        assert_eq!(shipments.1[0].columns, ["order_id", "line_no"]);
        let routines = session.routines(Some("shop"), &cancel).unwrap();
        let total = routines.iter().find(|r| r.name == "order_total").expect("order_total");
        assert_eq!(total.arity, Some(1), "{}", server.url);
        assert!(total.arguments.starts_with("o bigint"), "{}: {total:?}", server.url);
        let every = session.routines(None, &cancel).unwrap();
        assert!(every.iter().any(|r| r.builtin && r.name == "GROUP_CONCAT"));
        let mut schemas: Vec<String> = Vec::new();
        for info in &tables {
            if let Some(s) = &info.table.schema
                && !schemas.contains(s)
            {
                schemas.push(s.clone());
            }
        }
        let index = SchemaIndex::build(IndexParts {
            source: DataSourceId::new(),
            database: place.database.clone(),
            default_schema: place.schema.clone(),
            schemas,
            tables,
            columns,
            keys,
            routines: every,
            complete: true,
            ..IndexParts::default()
        });
        assert!(index.exists(&t("shop", "orders")) && index.exists(&t("analytics", "visits")));
        let flavor = Flavor::MySql;
        let suggest = |sql: &str| {
            let offset = sql.find('‸').unwrap();
            let text = sql.replace('‸', "");
            let scope = scope_in(&text, &lex(&text, flavor), offset, flavor);
            complete(&CompletionRequest {
                scope: &scope,
                index: Some(&index),
                flavor,
                schema: place.schema.as_deref(),
                limit: LIMIT,
            })
        };
        let in_analytics = suggest("SELECT * FROM analytics.‸");
        let names: Vec<&str> = in_analytics.items.iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"visits") && names.contains(&"daily"), "{names:?}");
        let columns = suggest("SELECT c.‸ FROM customers c");
        let names: Vec<&str> = columns
            .items
            .iter()
            .filter(|i| i.kind == SuggestionKind::Column)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(names[..3], ["id", "email", "name"], "{}", server.url);
        let join = suggest("SELECT * FROM orders o JOIN ‸");
        assert!(join.items.iter().any(|i| i.label.contains("customers")), "{}", server.url);
        // An enum column's value: its labels, in their order (48b).
        let values = suggest("SELECT * FROM orders o WHERE o.status = ‸");
        let values: Vec<&str> = values
            .items
            .iter()
            .filter(|i| i.kind == SuggestionKind::Value)
            .map(|i| i.insert.as_str())
            .collect();
        assert_eq!(values, ["'new'", "'paid'", "'shipped'", "'cancelled'"], "{}", server.url);
    });
}

#[test]
fn a_read_only_user_reads_and_writes_nothing() {
    each(|server| {
        let mut session = server.open("shop", Some("editor_reader")).expect("the reader connects");
        let tables = session.tables(&[]).unwrap();
        assert!(tables.iter().any(|i| i.table == t("shop", "customers")));
        let found = rows(session.as_mut(), "SELECT count(*) FROM customers");
        assert_eq!(found[0][0], Value::Int(2000));
        let refused = session.execute(
            "UPDATE countries SET name = name",
            &mut Collect::default(),
            &Cancel::default(),
        );
        assert!(refused.is_err(), "{}", server.url);
        // A read-only source's session refuses a write itself.
        let mut target = server.target.clone();
        target.options.insert(Target::READ_ONLY.into(), "true".into());
        let mut guarded = MySqlDriver.connect(&target, &server.secret).unwrap();
        let refused = guarded.execute(
            "UPDATE countries SET name = name",
            &mut Collect::default(),
            &Cancel::default(),
        );
        assert!(refused.is_err(), "{}: a read-only source wrote", server.url);
    });
}

#[test]
fn tls_is_used_when_the_server_has_it_and_refused_when_required_without() {
    each(|server| {
        let mut session = server.session("shop");
        let found = rows(session.as_mut(), "SHOW SESSION STATUS LIKE 'Ssl_cipher'");
        let encrypted = found.first().is_some_and(|r| !r[1].display().is_empty());
        let mut target = server.target.clone();
        target.options.insert("sslmode".into(), "require".into());
        let required = MySqlDriver.connect(&target, &server.secret).map(|_| ());
        // `prefer` encrypted exactly when the server can (`require` connects).
        assert_eq!(required.is_ok(), encrypted, "{}: {required:?}", server.url);
        if !encrypted {
            assert!(matches!(required, Err(DbError::Connect(_))), "{required:?}");
        }
        target.options.insert("sslmode".into(), "disable".into());
        let mut plain = MySqlDriver.connect(&target, &server.secret).unwrap();
        let found = rows(plain.as_mut(), "SHOW SESSION STATUS LIKE 'Ssl_cipher'");
        assert!(found.first().is_none_or(|r| r[1].display().is_empty()));
    });
}

#[test]
fn the_grids_changes_and_the_structures_ddl_run_as_each_version_takes_them() {
    use database::transaction::{alter_script, apply_script, run};
    use database::{CellChange, Change, ChangeSet, ColumnDef, IndexInfo, RowKey, SchemaChange};
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let dialect = session.dialect().unwrap();
        let cancel = Cancel::default();
        let countries = t("shop", "countries");
        let structure = session.structure(&countries).unwrap();
        // An update setting what's there: found (not "changed"), so one row, no conflict.
        let same = ChangeSet {
            table: countries.clone(),
            changes: vec![Change::Update {
                key: RowKey::Primary(vec![("code".into(), Value::Text("RO".into()))]),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("Romania".into()),
                    new: Value::Text("Romania".into()),
                }],
            }],
        };
        let script = apply_script(dialect.as_ref(), &same, Some(&structure)).unwrap();
        let applied = run(session.as_mut(), &script, &cancel, &mut |_, _| {}).unwrap();
        assert_eq!(applied.affected, 1, "{}", server.url);
        // One changed since it was read: a conflict, nothing applied.
        let stale = ChangeSet {
            table: countries,
            changes: vec![Change::Update {
                key: RowKey::Primary(vec![("code".into(), Value::Text("RO".into()))]),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("Not Romania".into()),
                    new: Value::Text("X".into()),
                }],
            }],
        };
        let script = apply_script(dialect.as_ref(), &stale, Some(&structure)).unwrap();
        let refused = run(session.as_mut(), &script, &cancel, &mut |_, _| {});
        assert!(matches!(refused, Err(DbError::Conflict(_))), "{}: {refused:?}", server.url);
        assert!(!session.in_transaction());
        assert_eq!(
            rows(session.as_mut(), "SELECT name FROM countries WHERE code = 'RO'")[0][0],
            Value::Text("Romania".into())
        );
        // The structure's DDL on a table of the session's own (temporary: the seed untouched).
        rows(session.as_mut(), "CREATE TEMPORARY TABLE editor_tmp (id int PRIMARY KEY, note text)");
        let tmp = t("shop", "editor_tmp");
        let mut changes = vec![
            SchemaChange::AddColumn {
                table: tmp.clone(),
                column: ColumnDef::new("size", "enum('S', 'M')")
                    .not_null()
                    .default(database::DefaultValue::Value(Value::Text("M".into()))),
            },
            SchemaChange::AlterColumn {
                table: tmp.clone(),
                column: "note".into(),
                to: ColumnDef::new("remark", "varchar(100)").not_null(),
            },
            SchemaChange::CreateIndex {
                table: tmp.clone(),
                index: IndexInfo {
                    name: "editor_tmp_size".into(),
                    columns: vec!["size".into(), "remark".into()],
                    unique: false,
                    implicit: false,
                    descending: vec![false, true],
                    predicate: None,
                },
            },
            SchemaChange::DropIndex { table: tmp.clone(), name: "editor_tmp_size".into() },
        ];
        let rename =
            SchemaChange::RenameColumn { table: tmp, from: "remark".into(), to: "memo".into() };
        if v.has(database::mysql::Feature::RenameColumn) {
            changes.push(rename);
        } else {
            assert!(dialect.schema(&rename).is_err(), "{}: needs the definition", server.url);
        }
        let script = alter_script(dialect.as_ref(), &changes).unwrap();
        run(session.as_mut(), &script, &cancel, &mut |_, _| {})
            .unwrap_or_else(|e| panic!("{}: {e}", server.url));
        let mut found = Collect::default();
        session.execute("SELECT * FROM editor_tmp", &mut found, &cancel).unwrap();
        let names: Vec<&str> = found.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"size"), "{}: {names:?}", server.url);
        assert!(names.contains(&"memo") || names.contains(&"remark"), "{names:?}");
        rows(session.as_mut(), "DROP TEMPORARY TABLE editor_tmp");
    });
}
