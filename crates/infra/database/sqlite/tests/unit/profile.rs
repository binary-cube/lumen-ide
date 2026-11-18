use std::path::Path;
use std::time::Duration;

use super::*;
use crate::tests::TempDir;
use database::profile::{ProfileOptions, derive};
use database::{Driver, Secret, Session, Target};

#[test]
fn every_form_of_a_plan_row_is_normalized() {
    type Case<'a> = (&'a str, OpKind, Option<&'a str>, Option<&'a str>, Option<&'a str>);
    let cases: &[Case] = &[
        ("SCAN users", OpKind::SeqScan, Some("users"), None, None),
        ("SCAN TABLE users", OpKind::SeqScan, Some("users"), None, None),
        ("SCAN TABLE users AS u", OpKind::SeqScan, Some("users"), None, None),
        (
            "SCAN users USING INDEX users_name",
            OpKind::IndexScan,
            Some("users"),
            Some("users_name"),
            None,
        ),
        (
            "SCAN users USING COVERING INDEX users_name",
            OpKind::IndexOnlyScan,
            Some("users"),
            Some("users_name"),
            None,
        ),
        (
            "SEARCH users USING INDEX users_name (name=?)",
            OpKind::IndexScan,
            Some("users"),
            Some("users_name"),
            Some("name=?"),
        ),
        (
            "SEARCH TABLE users USING INDEX i (a=? AND b>?)",
            OpKind::IndexScan,
            Some("users"),
            Some("i"),
            Some("a=? AND b>?"),
        ),
        (
            "SEARCH users USING COVERING INDEX users_name (name>?)",
            OpKind::IndexOnlyScan,
            Some("users"),
            Some("users_name"),
            Some("name>?"),
        ),
        (
            "SEARCH users USING INTEGER PRIMARY KEY (rowid=?)",
            OpKind::RowidLookup,
            Some("users"),
            None,
            Some("rowid=?"),
        ),
        (
            "SEARCH o USING AUTOMATIC COVERING INDEX (customer_id=?)",
            OpKind::AutoIndex,
            Some("o"),
            None,
            Some("customer_id=?"),
        ),
        (
            "SEARCH o USING AUTOMATIC PARTIAL COVERING INDEX (customer_id=?)",
            OpKind::AutoIndex,
            Some("o"),
            None,
            Some("customer_id=?"),
        ),
        ("USE TEMP B-TREE FOR ORDER BY", OpKind::TempBTree, None, None, None),
        ("USE TEMP B-TREE FOR GROUP BY", OpKind::TempBTree, None, None, None),
        ("USE TEMP B-TREE FOR DISTINCT", OpKind::TempBTree, None, None, None),
        ("USE TEMP B-TREE FOR RIGHT PART OF ORDER BY", OpKind::TempBTree, None, None, None),
        ("CORRELATED SCALAR SUBQUERY 1", OpKind::Other, None, None, None),
        ("SCALAR SUBQUERY 2", OpKind::Other, None, None, None),
        ("LIST SUBQUERY 3", OpKind::Other, None, None, None),
        ("CO-ROUTINE totals", OpKind::Coroutine, Some("totals"), None, None),
        ("MATERIALIZE totals", OpKind::Materialize, Some("totals"), None, None),
        ("MULTI-INDEX OR", OpKind::MultiIndexOr, None, None, None),
        ("INDEX 1", OpKind::Other, None, None, None),
        ("COMPOUND QUERY", OpKind::Compound, None, None, None),
        ("LEFT-MOST SUBQUERY", OpKind::Compound, None, None, None),
        ("UNION ALL", OpKind::Compound, None, None, None),
        ("BLOOM FILTER ON o (customer_id=?)", OpKind::BloomFilter, Some("o"), None, None),
        ("SCAN CONSTANT ROW", OpKind::Result, None, None, None),
        (
            "CREATE AUTOMATIC INDEX ON customers(city, name)",
            OpKind::Other,
            Some("customers"),
            None,
            None,
        ),
        ("SOMETHING NEW", OpKind::Other, None, None, None),
    ];
    for (text, op, object, index, cond) in cases {
        let n = sqlite_op(text);
        assert_eq!(n.op, *op, "{text}");
        assert_eq!(n.object.as_ref().map(|o| o.name.as_str()), *object, "{text}");
        assert_eq!(n.index.as_deref(), *index, "{text}");
        assert_eq!(
            n.conditions.iter().find(|c| c.kind == CondKind::IndexCond).map(|c| c.text.as_str()),
            *cond,
            "{text}"
        );
    }
    assert_eq!(
        sqlite_op("SCAN TABLE users AS u").object.and_then(|o| o.alias).as_deref(),
        Some("u")
    );
    assert_eq!(sqlite_op("CORRELATED SCALAR SUBQUERY 1").role, Some(ChildRole::SubPlan));
    assert_eq!(sqlite_op("CO-ROUTINE x").role, Some(ChildRole::Cte));
    assert_eq!(sqlite_op("UNION ALL").role, Some(ChildRole::Member));
    let temp = sqlite_op("USE TEMP B-TREE FOR GROUP BY");
    assert_eq!(temp.conditions[0], Condition::new(CondKind::GroupKey, "GROUP BY"));
    assert_eq!(sqlite_op("BLOOM FILTER ON o (customer_id=?)").conditions[0].text, "customer_id=?");
}

#[test]
fn the_plans_rows_are_a_tree_with_their_ids() {
    let rows = [
        (2, 0, "SCAN users".to_owned()),
        (5, 0, "SEARCH orders USING INDEX orders_user (user_id=?)".to_owned()),
        (9, 0, "CORRELATED SCALAR SUBQUERY 1".to_owned()),
        (12, 9, "SCAN teams".to_owned()),
        (20, 77, "USE TEMP B-TREE FOR ORDER BY".to_owned()),
    ];
    let (p, ids) = from_rows(&rows, ProfileDepth::Estimated);
    assert_eq!(ids, [2, 5, 9, 12, 20]);
    assert_eq!(p.roots.len(), 4, "a parent not there: at the top");
    assert_eq!(p.node(NodeId(3)).parent, Some(NodeId(2)));
    // Its projection is the plan read before (the Plan tab unchanged).
    assert_eq!(
        p.plan_nodes(),
        database::plan::from_query_plan(&rows)
            .into_iter()
            .map(|mut n| {
                fn strip(n: &mut database::PlanNode) {
                    n.detail = None;
                    n.children.iter_mut().for_each(strip);
                }
                strip(&mut n);
                n
            })
            .collect::<Vec<_>>()
    );
}

const SHOP: &str = "
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total REAL, status TEXT);
    CREATE INDEX orders_status ON orders (status);
";

/// A shop of 1 000 customers and 50 000 orders on a temporary file.
fn shop(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.file("shop.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SHOP).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    for i in 1..=1000 {
        conn.execute(
            "INSERT INTO customers (id, name, city) VALUES (?1, ?2, ?3)",
            params![i, format!("c{i}"), format!("city {}", i % 10)],
        )
        .unwrap();
    }
    for i in 1..=50_000 {
        let status = if i % 100 == 0 { "new" } else { "paid" };
        conn.execute(
            "INSERT INTO orders (id, customer_id, total, status) VALUES (?1, ?2, ?3, ?4)",
            params![i, i % 1000 + 1, f64::from(i % 97), status],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    path
}

fn open(path: &Path) -> Box<dyn Session> {
    let target = Target { database: path.to_string_lossy().into_owned(), ..Target::default() };
    crate::SqliteDriver.connect(&target, &Secret::default()).unwrap()
}

fn actual() -> ProfileOptions {
    ProfileOptions::ACTUAL
}

#[test]
fn a_scan_a_search_an_automatic_index_and_a_sort_are_measured() {
    let dir = TempDir::new("profile-measured");
    let path = shop(&dir);
    let mut session = open(&path);
    // A full scan: its rows counted, its counters, the scan's real rows.
    let p = session
        .profile("SELECT * FROM orders WHERE total > 90", &actual(), &Cancel::default())
        .unwrap();
    assert_eq!(p.engine.name, "SQLite");
    assert!(p.collected.actual && p.collected.engine_counters && p.collected.node_actuals_by_scan);
    let counters = p.statement.counters.unwrap();
    assert!(counters.fullscan_steps.unwrap() >= 49_000, "{counters:?}");
    assert!(counters.vm_steps.unwrap() > 0);
    let scan = &p.nodes[0];
    assert_eq!(scan.op, OpKind::SeqScan);
    assert_eq!(scan.actuals.loops, Some(1.0));
    assert_eq!(scan.actuals.rows, Some(50_000.0), "every row visited");
    assert!(scan.extra.iter().any(|e| e.key == "cycles"));
    assert!(p.statement.planning_ms.is_some() && p.statement.execution_ms.is_some());
    let returned = p.statement.rows_returned.unwrap();
    let mut count = database::Collect::default();
    session
        .execute("SELECT count(*) FROM orders WHERE total > 90", &mut count, &Cancel::default())
        .unwrap();
    assert_eq!(database::Value::Int(returned as i64), count.rows[0][0]);

    // A search through an index.
    let p = session
        .profile("SELECT * FROM orders WHERE status = 'new'", &actual(), &Cancel::default())
        .unwrap();
    assert_eq!(p.nodes[0].op, OpKind::IndexScan);
    assert_eq!(p.nodes[0].index.as_deref(), Some("orders_status"));
    assert_eq!(p.nodes[0].actuals.rows, Some(500.0));
    assert_eq!(p.statement.counters.unwrap().fullscan_steps, Some(0));

    // An automatic index (a join on a column without one) and a temporary B-tree.
    let sql = "SELECT a.name, b.name FROM customers a JOIN customers b ON b.city = a.city \
               WHERE a.id < 50 ORDER BY b.name";
    let p = session.profile(sql, &actual(), &Cancel::default()).unwrap();
    let ops: Vec<OpKind> = p.nodes.iter().map(|n| n.op).collect();
    assert!(ops.contains(&OpKind::AutoIndex), "{ops:?}");
    assert!(ops.contains(&OpKind::TempBTree), "{ops:?}");
    let c = p.statement.counters.unwrap();
    assert!(c.sorts.unwrap() > 0, "{c:?}");
    assert!(c.autoindex.unwrap() > 0, "{c:?}");
    // The inner loop ran once per outer customer, a hundred of the same city each time.
    let inner = p.nodes.iter().find(|n| n.op == OpKind::AutoIndex).unwrap();
    assert_eq!(inner.actuals.loops, Some(49.0), "{inner:?}");
    assert_eq!(inner.actuals.rows, Some(100.0), "{inner:?}");
    // Each element's time is its share of the statement's cycles: nothing past it.
    let report = derive(p);
    let total = report.totals.time_ms.unwrap();
    for d in &report.derived {
        assert!(d.inclusive_ms.unwrap_or(0.0) <= total * 1.0001, "{d:?} of {total}");
    }
    assert!(!report.hot_path.is_empty());
}

#[test]
fn a_profiled_write_leaves_the_table_as_it_was() {
    let dir = TempDir::new("profile-write");
    let path = shop(&dir);
    let mut session = open(&path);
    let count = |session: &mut Box<dyn Session>| {
        let mut rows = database::Collect::default();
        session
            .execute("SELECT count(*), sum(total) FROM orders", &mut rows, &Cancel::default())
            .unwrap();
        rows.rows[0].clone()
    };
    let before = count(&mut session);
    let p = session
        .profile("DELETE FROM orders WHERE status = 'paid'", &actual(), &Cancel::default())
        .unwrap();
    assert!(p.has_note(ProfileNote::RolledBack));
    assert_eq!(p.statement.rows_affected, Some(49_500));
    assert!(p.collected.undo_writes);
    assert_eq!(count(&mut session), before, "undone");
    assert!(!session.in_transaction());
    // Inside the user's transaction: undone to its savepoint, the transaction still open
    // with what it did before.
    session.begin(&database::TxOptions::default()).unwrap();
    session
        .execute(
            "UPDATE orders SET total = 0 WHERE id = 1",
            &mut database::Collect::default(),
            &Cancel::default(),
        )
        .unwrap();
    let p = session
        .profile("UPDATE orders SET total = total + 1", &actual(), &Cancel::default())
        .unwrap();
    assert!(p.has_note(ProfileNote::RolledBack));
    assert!(session.in_transaction());
    let mut rows = database::Collect::default();
    session
        .execute(
            "SELECT total FROM orders WHERE id IN (1, 2) ORDER BY id",
            &mut rows,
            &Cancel::default(),
        )
        .unwrap();
    assert_eq!(rows.rows, [[database::Value::Float(0.0)], [database::Value::Float(2.0)]]);
    session.rollback().unwrap();
    // What can't run in a transaction isn't run.
    assert!(matches!(
        session.profile("VACUUM", &actual(), &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
}

#[test]
fn the_time_limit_and_the_cancel_stop_a_profile() {
    let dir = TempDir::new("profile-stop");
    let path = shop(&dir);
    let mut session = open(&path);
    let slow =
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) SELECT count(*) FROM n";
    let limited = ProfileOptions { timeout: Some(Duration::from_millis(50)), ..actual() };
    let started = std::time::Instant::now();
    let stopped = session.profile(slow, &limited, &Cancel::default());
    let Err(DbError::Sql(e)) = &stopped else { panic!("{stopped:?}") };
    assert!(e.message.contains("time limit"), "{e:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(session.profile(slow, &actual(), &cancel).err(), Some(DbError::Cancelled));
    // The connection works on after them, scanstatus collected no more.
    let p = session.profile("SELECT 1", &ProfileOptions::ESTIMATED, &Cancel::default()).unwrap();
    assert_eq!(p.nodes[0].op, OpKind::Result);
    assert!(p.collected.estimated && !p.collected.actual);
    assert_eq!(p.raw.as_ref().map(|r| r.format), Some(RawFormat::Rows));
}

#[test]
fn the_tables_statistics_say_whether_analyze_ran() {
    let dir = TempDir::new("profile-stats");
    let path = shop(&dir);
    let mut session = open(&path);
    let tables = [TableRef::new("orders"), TableRef::new("customers")];
    let stats = session.table_stats(&tables, &Cancel::default()).unwrap();
    assert_eq!(stats.len(), 2);
    assert_eq!(stats[0].stats_present, Some(false), "no ANALYZE yet");
    assert_eq!(stats[0].indexes.len(), 1);
    assert_eq!(stats[0].indexes[0].name, "orders_status");
    assert_eq!(stats[0].indexes[0].columns, ["status"]);
    assert!(stats[0].extra.iter().any(|e| e.key == "file_bytes"));
    let mut rows = database::Collect::default();
    session.execute("ANALYZE", &mut rows, &Cancel::default()).unwrap();
    let stats = session.table_stats(&tables, &Cancel::default()).unwrap();
    assert_eq!(stats[0].stats_present, Some(true));
    assert_eq!(stats[0].live_rows, Some(50_000));
    if compiled_with(&Connection::open(&path).unwrap(), "ENABLE_DBSTAT_VTAB") {
        assert!(stats[0].table_bytes.is_some_and(|b| b > 0));
    }
}

#[test]
fn sqlite_is_built_with_the_scans_figures() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(
        compiled_with(&conn, "ENABLE_STMT_SCANSTATUS"),
        "LIBSQLITE3_FLAGS in .cargo/config.toml"
    );
}

/// The analysis (39h3) of a real SQLite profile, with the facts the console gathers: the
/// tables' structures and statistics read on the same connection.
fn analyzed(session: &mut Box<dyn Session>, sql: &str) -> Vec<database::profile::rules::Finding> {
    use database::console::PlannedStatement;
    use database::profile::rules::{
        Facts, RecentRuns, RuleContext, TableFacts, Thresholds, analyze, tables_of,
    };
    use database::profile::sql_map::SqlMap;
    let profile = session.profile(sql, &actual(), &Cancel::default()).unwrap();
    let flavor = database::sql::Flavor::Sqlite;
    let map = SqlMap::new(sql, flavor);
    let tables = tables_of(&profile, &map);
    let stats = session.table_stats(&tables, &Cancel::default()).unwrap_or_default();
    let facts = Facts {
        tables: tables
            .iter()
            .map(|t| TableFacts {
                structure: session.structure(t).ok(),
                stats: stats.iter().find(|s| s.table == *t).cloned(),
                ..TableFacts::new(t.clone())
            })
            .collect(),
        ..Facts::default()
    };
    let report = derive(profile);
    let tokens = database::sql::lex(sql, flavor);
    let statement = PlannedStatement {
        text: sql.to_owned(),
        range: 0..sql.len(),
        kind: database::sql::classify(sql, &tokens, flavor),
        bound: None,
    };
    let (recent, thresholds) = (RecentRuns::default(), Thresholds::DEFAULT);
    let cx = RuleContext::new(&report, &facts, &statement, flavor, &recent, &thresholds, false);
    analyze(&cx)
}

#[test]
fn a_real_scan_and_an_automatic_index_are_found_with_their_index() {
    let dir = TempDir::new("profile-rules");
    let path = shop(&dir);
    let mut session = open(&path);
    // A whole table read for one customer's orders.
    let sql = "SELECT * FROM orders WHERE customer_id = 7";
    let found = analyzed(&mut session, sql);
    let scan = found.iter().find(|f| f.rule == "sqlite-scan").expect("the scan");
    assert_eq!(
        scan.suggestion.as_ref().and_then(|s| s.sql()),
        Some("CREATE INDEX \"idx_orders_customer_id\" ON \"orders\" (\"customer_id\");")
    );
    assert_eq!(&sql[scan.sql_range.clone().unwrap()], "customer_id = 7");
    assert!(scan.quantity(database::profile::rules::EvidenceKey::Fullscan).is_some());
    // Through its index: nothing to say of a scan.
    let found = analyzed(&mut session, "SELECT * FROM orders WHERE status = 'new'");
    assert!(!found.iter().any(|f| f.rule == "sqlite-scan"));
    // A join on a column without an index: SQLite builds one each time.
    let sql = "SELECT a.name, b.name FROM customers a JOIN customers b ON b.city = a.city \
               WHERE a.id < 50 ORDER BY b.name";
    let found = analyzed(&mut session, sql);
    let auto = found.iter().find(|f| f.rule == "auto-index").expect("the automatic index");
    assert_eq!(
        auto.suggestion.as_ref().and_then(|s| s.sql()),
        Some("CREATE INDEX \"idx_customers_city\" ON \"customers\" (\"city\");")
    );
    // No ANALYZE yet and a join: the planner guesses.
    assert!(found.iter().any(|f| f.rule == "stale-stats"));
}
