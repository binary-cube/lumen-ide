//! The driver against every server of the local test matrix (docker/databases, seeded the
//! same everywhere): `EDITOR_TEST_POSTGRES_URLS`, comma-separated
//! `postgresql://editor:…@host:port/postgres` URLs (`scripts/test-databases.sh urls`
//! prints them). None set: nothing runs. Read-only: the seed's data is only read (what
//! writes goes in a transaction rolled back, or is refused).
//!
//! What a user's source sees: connected to `postgres` (empty) while the data lives in other
//! databases (`shop`, `analytics`) and schemas (`public`, `sales`, `audit`), some tables
//! owned by another role, one the user may not read, a database it may not connect to.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use database::completion::{CompletionRequest, LIMIT, SuggestionKind, complete};
use database::profile::{ProfileDepth, ProfileOptions};
use database::schema_index::{IndexParts, SchemaIndex};
use database::sql::{Flavor, lex, scope_in};
use database::{
    Cancel, Collect, DataSource, DataSourceId, DbError, Driver, Page, RunOptions, Scope, Secret,
    Session, TableKind, TableRef, Target, Value,
};
use db_postgres::{ID, Postgres, connection};

/// A server of the matrix: its URL's target and password.
struct Server {
    url: String,
    target: Target,
    secret: Secret,
}

fn servers() -> Vec<Server> {
    let Ok(urls) = std::env::var("EDITOR_TEST_POSTGRES_URLS") else { return Vec::new() };
    urls.split(',')
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|url| {
            let mut source = DataSource::new("matrix", ID, Scope::Project);
            let secret = database::fields::from_url(&connection(), url, &mut source)
                .expect("a PostgreSQL URL")
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
        Postgres.connect(&target, &self.secret)
    }

    fn session(&self, database: &str) -> Box<dyn Session> {
        self.open(database, None).unwrap_or_else(|e| panic!("{}: {database}: {e}", self.url))
    }
}

fn t(schema: &str, name: &str) -> TableRef {
    TableRef { schema: Some(schema.to_owned()), name: name.to_owned() }
}

/// `server_version_num`.
fn version(session: &mut dyn Session) -> u32 {
    match rows(session, "SHOW server_version_num").first().and_then(|r| r.first()) {
        Some(value) => value.display().parse().unwrap_or(0),
        None => 0,
    }
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
fn every_database_the_user_may_open_is_listed_the_default_one_empty() {
    each(|server| {
        let mut session = server.session("postgres");
        let databases = session.databases().unwrap();
        for wanted in ["postgres", "shop", "analytics"] {
            assert!(databases.iter().any(|d| d == wanted), "{}: {databases:?}", server.url);
        }
        // Not one it may not connect to, nor a template.
        assert!(!databases.iter().any(|d| d == "editor_private"), "{}: {databases:?}", server.url);
        assert!(!databases.iter().any(|d| d.starts_with("template")), "{databases:?}");
        // The user's case: `postgres` has no tables of its own — they're in the others.
        assert_eq!(session.tables(&[]).unwrap(), Vec::new(), "{}", server.url);
        let place = session.current().unwrap();
        assert_eq!(place.database.as_deref(), Some("postgres"));
        // A database it may not connect to is refused as such.
        let refused = server.open("editor_private", None).map(|_| ());
        assert!(matches!(refused, Err(DbError::Auth(_))), "{}: {refused:?}", server.url);
    });
}

#[test]
fn every_schema_and_table_is_listed_whoever_owns_it() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let schemas: Vec<String> =
            session.children(&[]).unwrap().into_iter().map(|n| n.name).collect();
        assert_eq!(schemas, ["audit", "public", "sales"], "{}", server.url);
        let tables = session.tables(&[]).unwrap();
        let kind = |table: &TableRef| tables.iter().find(|i| i.table == *table).map(|i| i.kind);
        for table in [
            t("public", "countries"),
            t("public", "customers"),
            t("public", "legacy_notes"),
            t("sales", "products"),
            t("sales", "orders"),
            t("sales", "order_lines"),
            t("sales", "shipments"),
            // Owned by another role: one readable, one not — both listed.
            t("audit", "events"),
            t("audit", "secrets"),
        ] {
            assert_eq!(kind(&table), Some(TableKind::Table), "{}: {table}", server.url);
        }
        assert_eq!(kind(&t("sales", "order_totals")), Some(TableKind::View));
        if v >= 90300 {
            assert_eq!(kind(&t("sales", "monthly_sales")), Some(TableKind::View), "{v}");
        }
        if v >= 100000 {
            // A partitioned table once, its partitions not.
            assert_eq!(kind(&t("audit", "log")), Some(TableKind::Table), "{v}");
            assert_eq!(kind(&t("audit", "log_2024")), None, "{v}");
        }
        assert!(tables.iter().all(|i| i.table.schema.as_deref() != Some("pg_catalog")));
        // The estimates (ANALYZEd by the seed).
        let customers = tables.iter().find(|i| i.table == t("public", "customers")).unwrap();
        assert_eq!(customers.rows, Some(2000), "{}", server.url);
        // One schema's.
        let sales = session.tables(&["sales".to_owned()]).unwrap();
        assert!(sales.iter().all(|i| i.table.schema.as_deref() == Some("sales")));
        assert!(sales.iter().any(|i| i.table.name == "order_totals"));
        // The explorer's nodes say the same.
        let audit: Vec<String> =
            session.children(&["audit".to_owned()]).unwrap().into_iter().map(|n| n.name).collect();
        assert!(audit.iter().any(|n| n == "secrets"), "{}: {audit:?}", server.url);
        let views: Vec<String> = session
            .children(&["sales".to_owned()])
            .unwrap()
            .into_iter()
            .filter(|n| n.kind == "view")
            .map(|n| n.name)
            .collect();
        let mut expected = vec!["order_totals"];
        if v >= 90300 {
            expected.insert(0, "monthly_sales");
        }
        assert_eq!(views, expected, "{}", server.url);
        let columns: Vec<String> = session
            .children(&["audit".to_owned(), "secrets".to_owned()])
            .unwrap()
            .into_iter()
            .filter(|n| n.kind == "column")
            .map(|n| n.name)
            .collect();
        assert_eq!(columns, ["id", "token"], "{}", server.url);
        // The other databases' own.
        let mut analytics = server.session("analytics");
        let listed: Vec<TableRef> =
            analytics.tables(&[]).unwrap().into_iter().map(|i| i.table).collect();
        assert_eq!(listed, [t("public", "visits"), t("reports", "daily")], "{}", server.url);
    });
}

#[test]
fn structures_keep_their_keys_indexes_and_generated_columns() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let lines = session.structure(&t("sales", "order_lines")).unwrap();
        assert_eq!(lines.primary_key(), ["order_id", "line_no"], "{}", server.url);
        // An enum column read with its table (a huge schema's second tier, 48h): its labels
        // in their order, kept as the console's completion reads them.
        let orders = session.structure(&t("sales", "orders")).unwrap();
        let status = orders.columns.iter().find(|c| c.name == "status").expect("status");
        let labels = ["new", "paid", "shipped", "cancelled"].map(String::from).to_vec();
        assert_eq!(status.values.as_ref(), Some(&labels), "{}", server.url);
        assert_eq!(database::ColumnBrief::from(status).values, Some(labels), "{}", server.url);
        assert!(orders.columns.iter().filter(|c| c.name != "status").all(|c| c.values.is_none()));
        let targets: Vec<String> = lines.foreign_keys.iter().map(|k| k.table.to_string()).collect();
        assert!(targets.contains(&"sales.orders".to_owned()), "{targets:?}");
        assert!(targets.contains(&"sales.products".to_owned()), "{targets:?}");
        let amount = lines.columns.iter().find(|c| c.name == "amount");
        if v >= 120000 {
            let generated = amount.and_then(|c| c.generated.as_ref()).expect("generated");
            assert!(generated.stored && generated.expression.contains("quantity"), "{v}");
        } else {
            assert!(amount.is_none());
        }
        // A composite foreign key, its columns in order.
        let shipments = session.structure(&t("sales", "shipments")).unwrap();
        let key = &shipments.foreign_keys[0];
        assert_eq!(key.columns, ["order_id", "line_no"]);
        assert_eq!(key.references, ["order_id", "line_no"]);
        assert_eq!(key.table, t("sales", "order_lines"));
        // Unique, expression and partial indexes.
        let customers = session.structure(&t("public", "customers")).unwrap();
        let index = |name: &str| customers.indexes.iter().find(|i| i.name == name);
        let email = index("customers_email_key").expect("the unique key's index");
        assert!(email.unique && email.implicit);
        assert_eq!(index("customers_lower_name").unwrap().columns, ["lower(name)"]);
        let vip = index("customers_vip_created").unwrap();
        assert_eq!(vip.predicate.as_deref(), Some("vip"));
        assert_eq!(vip.descending, [true]);
        assert!(customers.columns.iter().any(|c| c.name == "id" && c.auto_increment));
        // A table the user may not read: its structure all the same.
        let secrets = session.structure(&t("audit", "secrets")).unwrap();
        assert_eq!(secrets.primary_key(), ["id"]);
        if v >= 100000 {
            let coupons = session.structure(&t("sales", "coupons")).unwrap();
            assert!(coupons.columns[0].auto_increment, "identity: {v}");
        }
    });
}

#[test]
fn the_grid_reads_pages_by_key_and_refuses_what_it_may_not_read() {
    each(|server| {
        let mut session = server.session("shop");
        let customers = t("public", "customers");
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
        session.read(&t("sales", "order_lines"), &page, &mut lines, &Cancel::default()).unwrap();
        assert_eq!(lines.rows.len(), 10);
        // Another role's table, readable; one not.
        let mut events = Collect::default();
        session
            .read(&t("audit", "events"), &Page::first(10), &mut events, &Cancel::default())
            .unwrap();
        assert_eq!(events.rows.len(), 10);
        let refused = session.read(
            &t("audit", "secrets"),
            &Page::first(10),
            &mut Collect::default(),
            &Cancel::default(),
        );
        let refused = refused.expect_err("not readable");
        assert_eq!(refused.sql_error().and_then(|e| e.sqlstate.as_deref()), Some("42501"));
        // An enum, an array, json.
        let mut orders = Collect::default();
        session
            .read(&t("sales", "orders"), &Page::first(3), &mut orders, &Cancel::default())
            .unwrap();
        assert!(matches!(&orders.rows[0][2], Value::Other(s) if s == "paid"), "{:?}", orders.rows);
    });
}

#[test]
fn the_console_runs_fetches_explains_and_profiles() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let cancel = Cancel::default();
        // A page at a time from a cursor.
        let options = RunOptions { fetch_limit: 3000, keep_cursor: true };
        let mut first = Collect::default();
        let outcome = session
            .run("SELECT * FROM sales.order_lines ORDER BY 1, 2", &options, &mut first, &cancel)
            .unwrap();
        assert!(outcome.truncated && first.rows.len() == 3000, "{}", server.url);
        let mut total = first.rows.len();
        let mut next = outcome.more;
        while let Some(token) = next {
            let mut page = Collect::default();
            let outcome = session.fetch(&token, 3000, &mut page, &cancel).unwrap();
            total += page.rows.len();
            next = outcome.more;
        }
        assert_eq!(total, 8000);
        assert!(!session.in_transaction());
        // The schema switched; where the session is.
        session.set_schema("sales").unwrap();
        let place = session.current().unwrap();
        assert_eq!(
            (place.database.as_deref(), place.schema.as_deref()),
            (Some("shop"), Some("sales"))
        );
        let found = rows(session.as_mut(), "SELECT count(*) FROM orders");
        assert_eq!(found[0][0], Value::Int(4000));
        // A plan, and one as it ran.
        let query = "SELECT c.name, count(*) FROM public.customers c \
                     JOIN sales.orders o ON o.customer_id = c.id GROUP BY c.name";
        let plan = session.explain(query, false, &cancel).unwrap();
        assert!(!plan.is_empty(), "{}", server.url);
        let ran = session.explain(query, true, &cancel).unwrap();
        assert!(
            ran.iter().flat_map(|n| n.walk()).any(|n| n.actual_rows.is_some()),
            "{}",
            server.url
        );
        let profile = session
            .profile(query, &ProfileOptions::of(ProfileDepth::Actual { timing: true }), &cancel)
            .unwrap();
        assert!(!profile.nodes.is_empty());
        assert_eq!(profile.engine.version_num, Some(v), "{}", server.url);
        assert_eq!(profile.statement.rows_returned, Some(2000), "{}", server.url);
        // A write profiled as it runs is undone.
        let write = "UPDATE sales.products SET price = price + 1";
        session
            .profile(write, &ProfileOptions::of(ProfileDepth::Actual { timing: false }), &cancel)
            .unwrap();
        let price = rows(session.as_mut(), "SELECT price::text FROM sales.products WHERE id = 1");
        assert_eq!(price[0][0], Value::Text("3.99".into()), "{}", server.url);
        // The tables' statistics.
        let stats = session
            .table_stats(&[t("public", "customers"), t("sales", "orders")], &cancel)
            .unwrap();
        assert_eq!(stats.len(), 2, "{}", server.url);
        let customers = stats.iter().find(|s| s.table == t("public", "customers")).unwrap();
        assert_eq!(customers.live_rows, Some(2000), "{}: {v}", server.url);
        assert!(customers.total_bytes.is_some_and(|b| b > 0));
        assert!(!customers.indexes.is_empty());
    });
}

#[test]
fn the_catalog_in_bulk_builds_a_schema_index_that_completes() {
    each(|server| {
        let mut session = server.session("shop");
        let v = version(session.as_mut());
        let cancel = Cancel::default();
        let place = session.current().unwrap();
        let tables = session.tables(&[]).unwrap();
        let columns = session.catalog_columns(None, &cancel).unwrap();
        let customers = columns.iter().find(|(t, _)| *t == self::t("public", "customers"));
        let (_, customer_columns) = customers.expect("the customers' columns");
        let email = customer_columns.iter().find(|c| c.name == "email").unwrap();
        assert_eq!(email.comment.as_deref(), Some("Where we write to them"));
        assert_eq!(customer_columns[0].key_position, Some(1));
        // Another role's table, and one not readable: their columns all the same.
        assert!(columns.iter().any(|(t, _)| *t == self::t("audit", "secrets")));
        let keys = session.catalog_keys(None, &cancel).unwrap();
        let shipments = keys.iter().find(|(t, _)| *t == self::t("sales", "shipments")).unwrap();
        assert_eq!(shipments.1[0].columns, ["order_id", "line_no"]);
        let routines = session.routines(Some("sales"), &cancel).unwrap();
        assert!(routines.iter().any(|r| r.name == "order_total"), "{}", server.url);
        let readable = session.catalog_relations(&cancel).unwrap();
        assert!(readable.iter().any(|t| t.name == "customers_id_seq"), "{}", server.url);
        if v >= 100000 {
            assert!(readable.contains(&self::t("audit", "log_2024")), "{v}");
        }
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
            routines,
            readable,
            complete: true,
            ..IndexParts::default()
        });
        assert!(index.exists(&t("sales", "orders")) && index.exists(&t("audit", "secrets")));
        let flavor = Flavor::Postgres { version: Some(v) };
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
        let in_sales = suggest("SELECT * FROM sales.‸");
        let names: Vec<&str> = in_sales.items.iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"orders") && names.contains(&"order_lines"), "{names:?}");
        let columns = suggest("SELECT c.‸ FROM customers c");
        let names: Vec<&str> = columns
            .items
            .iter()
            .filter(|i| i.kind == SuggestionKind::Column)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(names[..3], ["id", "email", "name"], "{}", server.url);
        // A join's condition from the foreign key.
        let join = suggest("SELECT * FROM sales.orders o JOIN ‸");
        assert!(join.items.iter().any(|i| i.label.contains("customers")), "{}", server.url);
        // An enum column's value: its labels, in their order (48b).
        let values = suggest("SELECT * FROM sales.orders o WHERE o.status = ‸");
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
        assert!(tables.iter().any(|i| i.table == t("audit", "events")));
        let found = rows(session.as_mut(), "SELECT count(*) FROM public.customers");
        assert_eq!(found[0][0], Value::Int(2000));
        let refused = session.execute(
            "UPDATE public.countries SET name = name",
            &mut Collect::default(),
            &Cancel::default(),
        );
        assert!(refused.is_err(), "{}", server.url);
    });
}
