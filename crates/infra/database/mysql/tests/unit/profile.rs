//! MySQL's and MariaDB's plans read into the normalized profile, from their real answers
//! (`tests/data`, captured on the local matrix's servers: MySQL 5.7, 8.0, 8.4, MariaDB
//! 10.11, 11.4, 11.8), without a server.

use super::*;
use database::profile::{derive, export};

fn data(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

const RAN: ProfileDepth = ProfileDepth::Actual { timing: true };

fn find<'a>(p: &'a Profile, name: &str) -> &'a ProfileNode {
    p.nodes.iter().find(|n| n.name.starts_with(name)).unwrap_or_else(|| panic!("{name}: {p:?}"))
}

fn on<'a>(p: &'a Profile, object: &str) -> &'a ProfileNode {
    p.nodes
        .iter()
        .find(|n| n.object.as_ref().is_some_and(|o| o.name == object))
        .unwrap_or_else(|| panic!("{object}"))
}

#[test]
fn what_each_version_profiles() {
    let old = caps(ServerVersion::mysql(50744));
    assert!(old.estimated && !old.actual && !old.undo_writes && old.table_stats);
    let new = caps(ServerVersion::mysql(80018));
    assert!(new.actual && new.node_timing && new.engine_counters && new.undo_writes);
    assert!(!caps(ServerVersion::mysql(80017)).actual);
    assert!(caps(ServerVersion::mariadb(101106)).actual);
    assert!(!new.buffers && !new.wal && !new.jit, "nothing it doesn't have");
}

#[test]
fn mysqls_json_plan_is_read_with_every_key_kept() {
    let p = parse_json(&data("mysql-8.4-explain.json"), ProfileDepth::Estimated).unwrap();
    let grouping = &p.nodes[p.roots[0].index()];
    assert_eq!(grouping.op, OpKind::Aggregate(AggStrategy::Hashed), "a temporary table's");
    let join = find(&p, "Nested loop");
    assert_eq!((join.op, join.join), (OpKind::NestedLoop, Some(JoinKind::Inner)));
    let c = on(&p, "c");
    assert_eq!(c.op, OpKind::SeqScan);
    assert_eq!(c.estimates.rows, Some(2000.0));
    assert!(c.condition(&CondKind::Filter).is_some_and(|f| f.contains("vip")));
    assert!(c.extra.iter().any(|e| e.key == "filtered" && e.value.number() == Some(10.0)));
    assert!(c.extra.iter().any(|e| e.key == "used_columns"), "an unknown key kept");
    let o = on(&p, "o");
    assert_eq!((o.op, o.index.as_deref()), (OpKind::IndexOnlyScan, Some("orders_customer")));
    assert_eq!(o.role, Some(ChildRole::Inner));
    assert!(o.estimates.total_cost.is_some(), "its prefix cost");
    assert!(p.statement.extra.iter().any(|e| e.key == "query_cost"));
    assert!(export::to_text(&p).contains("ALL on c"));
}

#[test]
fn mysql_57s_subqueries_and_unions_are_sub_plans() {
    let p = parse_json(&data("mysql-5.7-subqueries.json"), ProfileDepth::Estimated).unwrap();
    // An ordering by a sort, a derived table's union under its table, a dependent
    // subquery of the select list.
    assert_eq!(p.nodes[p.roots[0].index()].op, OpKind::Sort);
    let union = find(&p, "Union");
    assert_eq!(union.op, OpKind::Append);
    let members: Vec<&ProfileNode> = union
        .children
        .iter()
        .map(|c| p.node(*c))
        .filter(|n| n.role == Some(ChildRole::Member))
        .collect();
    assert_eq!(members.len(), 2);
    let derived = find(&p, "Subquery");
    assert_eq!(derived.role, Some(ChildRole::Subquery));
    let dependent = find(&p, "Select #2");
    assert_eq!(dependent.role, Some(ChildRole::SubPlan));
    assert!(dependent.extra.iter().any(|e| e.key == "dependent"));
    assert!(p.nodes.iter().all(|n| !n.name.is_empty()));
}

#[test]
fn mariadbs_analyze_says_what_each_table_did_per_loop() {
    let p = parse_json(&data("mariadb-11.4-analyze.json"), RAN).unwrap();
    assert!(p.statement.planning_ms.is_some() && p.statement.execution_ms.is_some());
    let sort = &p.nodes[p.roots[0].index()];
    assert_eq!(sort.op, OpKind::Sort);
    assert!(sort.condition(&CondKind::SortKey).is_some_and(|k| k.contains("name")));
    assert_eq!(sort.actuals.rows, Some(1800.0));
    let c = on(&p, "c");
    assert_eq!((c.actuals.loops, c.op), (Some(1.0), OpKind::SeqScan));
    // 2000 rows read, 90% kept by its condition.
    assert_eq!(c.actuals.rows, Some(1800.0));
    assert_eq!(c.actuals.rows_removed_by_filter, Some(200.0));
    let o = on(&p, "o");
    assert_eq!(o.actuals.loops, Some(1800.0));
    // r_table_time_ms + r_other_time_ms are all loops': per loop here.
    let all = o
        .extra
        .iter()
        .filter(|e| e.key.ends_with("_time_ms"))
        .filter_map(|e| e.value.number())
        .sum::<f64>();
    assert!((o.actuals.total_ms.unwrap() * 1800.0 - all).abs() < 1e-6);
    assert!(o.extra.iter().any(|e| e.key == "r_engine_stats"));
    let report = derive(p.clone());
    assert!(report.totals.time_ms.is_some());
}

#[test]
fn mariadbs_estimated_plan_and_its_subqueries() {
    let p = parse_json(&data("mariadb-10.11-explain.json"), ProfileDepth::Estimated).unwrap();
    assert_eq!(on(&p, "c").estimates.rows, Some(12.0));
    assert!(p.nodes.iter().all(|n| n.actuals.rows.is_none()));
    let p = parse_json(&data("mariadb-11.8-subqueries.json"), RAN).unwrap();
    assert!(p.nodes.iter().any(|n| n.op == OpKind::Append));
    assert!(p.nodes.iter().any(|n| n.role == Some(ChildRole::Member)));
    let customers: Vec<&ProfileNode> = p
        .nodes
        .iter()
        .filter(|n| n.object.as_ref().is_some_and(|o| o.name == "customers"))
        .collect();
    assert_eq!(customers.len(), 2, "both parts of the union");
}

#[test]
fn mysqls_tree_as_it_ran_is_read_line_by_line() {
    let p = parse_tree(&data("mysql-8.4-analyze.txt"), RAN).unwrap();
    assert_eq!(p.roots.len(), 1);
    let root = &p.nodes[p.roots[0].index()];
    assert_eq!(root.op, OpKind::SeqScan, "the temporary table read");
    assert!(root.object.is_none(), "the server's own table");
    assert_eq!(root.actuals.rows, Some(1800.0));
    assert_eq!(p.statement.execution_ms, root.actuals.total_ms);
    let agg = find(&p, "Aggregate using temporary table");
    assert_eq!(agg.op, OpKind::Aggregate(AggStrategy::Hashed));
    let scan = find(&p, "Table scan on c");
    assert_eq!(scan.op, OpKind::SeqScan);
    assert_eq!((scan.estimates.total_cost, scan.estimates.rows), (Some(204.0), Some(2000.0)));
    assert_eq!(scan.actuals.rows, Some(2000.0));
    // The filter above it gave it its condition and what it removed.
    assert!(scan.condition(&CondKind::Filter).is_some_and(|c| c.contains("vip")));
    assert_eq!(scan.actuals.rows_removed_by_filter, Some(200.0));
    let lookup = find(&p, "Covering index lookup on o");
    assert_eq!(
        (lookup.op, lookup.index.as_deref()),
        (OpKind::IndexOnlyScan, Some("orders_customer"))
    );
    assert_eq!(lookup.condition(&CondKind::IndexCond), Some("(customer_id=c.id)"));
    assert_eq!(lookup.actuals.loops, Some(1800.0));
    let report = derive(p);
    assert!(report.totals.time_ms.is_some());
}

#[test]
fn hash_joins_ranges_and_subqueries_of_the_tree() {
    let p = parse_tree(&data("mysql-8.0-hash-join.txt"), RAN).unwrap();
    let join = find(&p, "Inner hash join");
    assert_eq!((join.op, join.join), (OpKind::HashJoin, Some(JoinKind::Inner)));
    assert!(join.condition(&CondKind::HashCond).is_some());
    assert_eq!(find(&p, "Hash").op, OpKind::Hash);
    let range = find(&p, "Index range scan on a");
    assert_eq!(
        (range.op, range.condition(&CondKind::IndexCond)),
        (OpKind::IndexScan, Some("(id < 100)"))
    );
    let p = parse_tree(&data("mysql-8.4-subqueries.txt"), RAN).unwrap();
    assert_eq!(p.nodes[p.roots[0].index()].op, OpKind::Limit);
    let reverse = find(&p, "Index scan on o");
    assert_eq!(reverse.index.as_deref(), Some("orders_recent"));
    let fraction = find(&p, "Single-row covering index lookup on x");
    assert_eq!(fraction.actuals.rows, Some(0.149), "rows per loop with decimals");
    let sub = find(&p, "Select #2");
    assert_eq!(sub.role, Some(ChildRole::SubPlan));
    assert_eq!(find(&p, "Union materialize").op, OpKind::Append);
}

#[test]
fn a_tree_lines_figures_are_read_whatever_the_version_writes() {
    let line = tree_line(
        "Filter: (a.x = 'b (c)')  (cost=1.5 rows=2) (actual time=0.1..0.2 rows=3 loops=4)",
    );
    assert_eq!(line.words, "Filter: (a.x = 'b (c)')");
    assert_eq!((line.total_cost, line.rows), (Some(1.5), Some(2.0)));
    assert_eq!(line.actual, Some((0.1, 0.2, 3.0, 4.0)));
    let range = tree_line("Index lookup on t using i (a=1)  (cost=0.25..0.5 rows=1)");
    assert_eq!((range.startup_cost, range.total_cost), (Some(0.25), Some(0.5)));
    let never = tree_line("Table scan on t  (cost=1 rows=1) (never executed)");
    assert!(never.never && never.actual.is_none());
    let bare = tree_line("Hash");
    assert_eq!(bare, TreeLine { words: "Hash".into(), ..TreeLine::default() });
    let p = parse_tree("-> Table scan on t  (cost=1 rows=1) (never executed)", RAN).unwrap();
    assert!(p.nodes[0].actuals.never_executed);
    assert!(parse_tree("", RAN).is_err());
    assert!(parse_json("{}", RAN).is_err());
    assert!(parse_json("not json", RAN).is_err());
}

#[test]
fn a_newer_versions_keys_are_kept_as_extras() {
    let json = r#"{"query_block": {"select_id": 1, "brand_new": {"x": 1},
        "table": {"table_name": "t", "access_type": "ALL", "rows_examined_per_scan": 5,
        "future_figure": 1.5}}}"#;
    let p = parse_json(json, ProfileDepth::Estimated).unwrap();
    let t = on(&p, "t");
    assert!(t.extra.iter().any(|e| e.key == "future_figure"));
    assert!(p.statement.extra.iter().any(|e| e.key == "brand_new"));
    // A write's table.
    let json = r#"{"query_block": {"select_id": 1, "table": {"update": true,
        "table_name": "products", "access_type": "range", "key": "PRIMARY"}}}"#;
    let p = parse_json(json, ProfileDepth::Estimated).unwrap();
    assert_eq!(p.nodes[0].op, OpKind::ModifyTable(WriteKind::Update));
}

#[test]
fn the_sessions_counters_are_what_the_run_added() {
    let before = vec![("Select_scan".to_owned(), 3), ("Handler_read_rnd_next".to_owned(), 100)];
    let after = vec![
        ("Select_scan".to_owned(), 4),
        ("Handler_read_rnd_next".to_owned(), 2101),
        ("Handler_read_key".to_owned(), 1800),
        ("Sort_merge_passes".to_owned(), 0),
    ];
    let (counters, extra) = counters(&before, &after);
    assert_eq!(counters.select_scan, Some(1));
    assert_eq!(counters.rows_examined, Some(2001 + 1800));
    assert_eq!(counters.sort_merge_passes, Some(0));
    assert_eq!(counters.tmp_disk_tables, None, "not read: unknown, not zero");
    assert!(extra.iter().all(|e| e.group.as_deref() == Some("Session status")));
    assert!(status_sql().contains("'Select_scan'"));
    let rows = vec![vec![Some("Select_scan".to_owned()), Some("7".to_owned())]];
    assert_eq!(status_values(&rows), [("Select_scan".to_owned(), 7)]);
}

#[test]
fn what_cant_be_undone_isnt_run() {
    use database::sql::{StatementKind, TxControl};
    assert!(refused(StatementKind::Transaction(TxControl::Begin), "BEGIN").is_some());
    assert!(
        refused(
            crate::console::statement("ALTER TABLE t ADD x int").kind,
            "ALTER TABLE t ADD x int"
        )
        .is_some()
    );
    assert!(refused(StatementKind::Query, "SELECT 1").is_none());
    assert!(
        refused(crate::console::statement("UPDATE t SET a = 1").kind, "UPDATE t SET a = 1")
            .is_none()
    );
}

#[test]
fn performance_schemas_record_of_the_statement_is_read() {
    let sql = history_sql("EXPLAIN ANALYZE");
    assert!(sql.contains("events_statements_history") && sql.contains("LIKE 'EXPLAIN ANALYZE%'"));
    let row: Vec<Option<String>> = ["2000", "1", "1", "0", "5000000", "0", "0", "1"]
        .iter()
        .map(|v| Some((*v).to_owned()))
        .collect();
    let (examined, extra) = history_row(&row);
    assert_eq!(examined, Some(2000));
    let get = |key: &str| extra.iter().find(|e| e.key == key).map(|e| e.value.clone());
    assert_eq!(get("NO_INDEX_USED"), Some(ExtraValue::Bool(true)));
    assert_eq!(get("LOCK_TIME"), Some(ExtraValue::Quantity(Quantity::new(0.005, Unit::Millis))));
    assert!(extra.iter().all(|e| e.group.as_deref() == Some("performance_schema")));
    assert_eq!(history_row(&[]).0, None);
}
