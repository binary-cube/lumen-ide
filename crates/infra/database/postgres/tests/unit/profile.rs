use std::path::{Path, PathBuf};

use super::*;
use database::profile::{ProfileReport, derive, export};

fn plans(json: &str) -> Vec<database::PlanNode> {
    parse(json, BLOCK_SIZE).unwrap().plan_nodes()
}

#[test]
fn an_explain_analyze_saved_as_json_is_its_tree() {
    let plans = plans(include_str!("../data/explain-analyze.json"));
    assert_eq!(plans.len(), 1);
    let join = &plans[0];
    assert_eq!(join.operation, "Hash Join (Left)");
    assert_eq!(join.detail.as_deref(), Some("Hash Cond: (o.customer_id = c.id)"));
    assert_eq!((join.startup_cost, join.total_cost), (Some(1.07), Some(2.21)));
    assert_eq!((join.plan_rows, join.actual_rows, join.loops), (Some(5.0), Some(5.0), Some(1.0)));
    assert_eq!(join.actual_time_ms, Some(0.04));
    let scan = &join.children[0];
    assert_eq!(scan.operation, "Seq Scan");
    assert_eq!(scan.object.as_deref(), Some("orders o"));
    assert_eq!(scan.detail.as_deref(), Some("Filter: (total > '0'::numeric)"));
    let index = &join.children[1].children[0];
    assert_eq!(index.object.as_deref(), Some("customers c"));
    assert_eq!(index.detail.as_deref(), Some("using customers_pkey; Index Cond: (id > 0)"));
    assert_eq!(join.walk().len(), 4);
}

#[test]
fn a_plain_plan_has_no_times_and_a_broken_one_is_an_error() {
    let plain = r#"[{"Plan": {"Node Type": "Result", "Startup Cost": 0.0, "Total Cost": 0.01,
        "Plan Rows": 1, "Plan Width": 4}}]"#;
    let p = plans(plain);
    assert_eq!(p[0].operation, "Result");
    assert_eq!((p[0].actual_rows, p[0].actual_time_ms), (None, None));
    // A schema-qualified relation (`VERBOSE`) and a function scan.
    let verbose = r#"[{"Plan": {"Node Type": "Nested Loop", "Join Type": "Inner", "Plans": [
        {"Node Type": "Seq Scan", "Relation Name": "t", "Schema": "s", "Alias": "t"},
        {"Node Type": "Function Scan", "Function Name": "generate_series", "Alias": "g"}]}}]"#;
    let p = plans(verbose);
    assert_eq!(p[0].operation, "Nested Loop", "an inner join says no join type");
    assert_eq!(p[0].children[0].object.as_deref(), Some("s.t"));
    assert_eq!(p[0].children[1].object.as_deref(), Some("generate_series g"));
    assert!(matches!(parse("not json", BLOCK_SIZE), Err(DbError::Query(_))));
    assert!(matches!(parse("[{}]", BLOCK_SIZE), Err(DbError::Query(_))));
}

/// `EXPLAIN (FORMAT JSON)` without `ANALYZE`, from a saved answer: a tree of estimates whose
/// costliest branch is the planner's cost, and whose table shows no time.
#[test]
fn a_saved_estimated_plan_reads_as_a_tree() {
    let plans = plans(include_str!("../data/explain.json"));
    assert_eq!(plans.len(), 1);
    let flat = database::plan::flatten(&plans);
    let ops: Vec<(usize, &str)> =
        flat.iter().map(|f| (f.depth, f.node.operation.as_str())).collect();
    assert_eq!(
        ops,
        [
            (0, "Limit"),
            (1, "Sort"),
            (2, "Aggregate"),
            (3, "Hash Join"),
            (4, "Seq Scan"),
            (4, "Hash"),
            (5, "Seq Scan"),
        ]
    );
    assert_eq!(flat[4].node.object.as_deref(), Some("public.orders"));
    assert_eq!(flat[1].node.detail.as_deref(), Some("Sort Key: (sum(o.total)) DESC"));
    let columns = database::plan::PlanColumns::of(&plans);
    assert!(columns.cost && columns.rows && columns.detail);
    assert!(!columns.time && !columns.actual_rows && !columns.loops);
    assert_eq!(columns.measure(), Some(database::plan::Measure::Cost));
    // Down the costliest children: the orders' scan (140) over the customers' hash (20).
    assert_eq!(database::plan::hot_path(&plans), [0, 1, 2, 3, 4]);
    let text = database::plan::to_text(&plans);
    assert!(text.starts_with("Limit  (cost=245.32..245.35 rows=10)\n"), "{text}");
    assert!(
        text.contains(
            "\n        Seq Scan on public.orders (Filter: (status <> 'cancelled'::text))"
        )
    );
}

/// The analyzed plan's costliest branch follows the time, not the cost.
#[test]
fn an_analyzed_plan_is_weighed_by_its_time() {
    let plans = plans(include_str!("../data/explain-analyze.json"));
    let columns = database::plan::PlanColumns::of(&plans);
    assert_eq!(columns.measure(), Some(database::plan::Measure::Time));
    // The hash (0.012 ms) took longer than the orders' scan (0.01 ms).
    assert_eq!(database::plan::hot_path(&plans), [0, 2, 3]);
}

/// What each major version is asked, from 9.2 to 18.
#[test]
fn each_version_is_asked_what_it_takes() {
    let estimated = ProfileOptions::ESTIMATED;
    let actual = ProfileOptions::ACTUAL;
    let untimed = ProfileOptions::of(ProfileDepth::Actual { timing: false });
    let cases: &[(u32, &ProfileOptions, bool, &str)] = &[
        (90_200, &estimated, false, "EXPLAIN (FORMAT JSON) "),
        (90_200, &actual, true, "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "),
        (90_200, &untimed, false, "EXPLAIN (ANALYZE, TIMING OFF, BUFFERS, FORMAT JSON) "),
        (90_400, &actual, false, "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "),
        (90_600, &estimated, false, "EXPLAIN (FORMAT JSON) "),
        (100_000, &estimated, false, "EXPLAIN (SUMMARY, FORMAT JSON) "),
        (110_000, &actual, false, "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) "),
        (120_000, &estimated, false, "EXPLAIN (SETTINGS, SUMMARY, FORMAT JSON) "),
        (120_000, &actual, true, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON) "),
        (130_000, &estimated, false, "EXPLAIN (BUFFERS, SETTINGS, SUMMARY, FORMAT JSON) "),
        (130_000, &actual, true, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, WAL, FORMAT JSON) "),
        (130_000, &actual, false, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON) "),
        (140_000, &actual, true, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, WAL, FORMAT JSON) "),
        (150_000, &estimated, false, "EXPLAIN (BUFFERS, SETTINGS, SUMMARY, FORMAT JSON) "),
        (160_000, &actual, false, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, FORMAT JSON) "),
        (170_000, &estimated, false, "EXPLAIN (BUFFERS, SETTINGS, SUMMARY, MEMORY, FORMAT JSON) "),
        (170_000, &actual, true, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, WAL, MEMORY, FORMAT JSON) "),
        (180_000, &actual, false, "EXPLAIN (ANALYZE, BUFFERS, SETTINGS, MEMORY, FORMAT JSON) "),
    ];
    for (version, o, writes, expected) in cases {
        assert_eq!(options(*version, o, *writes), *expected, "{version} {o:?}");
    }
    // Asked for more: VERBOSE, SERIALIZE (17+); BUFFERS turned off where it's the default.
    let more = ProfileOptions { verbose: true, serialize: true, ..ProfileOptions::ACTUAL };
    assert_eq!(
        options(170_000, &more, false),
        "EXPLAIN (ANALYZE, BUFFERS, VERBOSE, SETTINGS, MEMORY, SERIALIZE TEXT, FORMAT JSON) "
    );
    assert_eq!(
        options(160_000, &more, false),
        "EXPLAIN (ANALYZE, BUFFERS, VERBOSE, SETTINGS, FORMAT JSON) "
    );
    let bare = ProfileOptions { buffers: false, settings: false, ..ProfileOptions::ACTUAL };
    assert_eq!(
        options(180_000, &bare, false),
        "EXPLAIN (ANALYZE, BUFFERS OFF, MEMORY, FORMAT JSON) "
    );
    assert_eq!(options(170_000, &bare, false), "EXPLAIN (ANALYZE, MEMORY, FORMAT JSON) ");
    // What each one's profiles can hold.
    assert!(!caps(90_200).parallel && caps(90_600).parallel);
    assert!(!caps(100_000).jit && caps(110_000).jit);
    assert!(!caps(110_000).settings && caps(120_000).settings);
    assert!(!caps(120_000).wal && caps(130_000).wal);
    assert!(caps(90_200).actual && caps(90_200).undo_writes && !caps(180_000).engine_counters);
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/profile").join(name)
}

/// Each fixture's profile as text against its golden file (`EDITOR_UPDATE_GOLDEN=1` writes
/// them anew).
fn golden(name: &str) -> ProfileReport {
    let json = std::fs::read_to_string(data(&format!("{name}.json"))).unwrap();
    let profile = parse(&json, BLOCK_SIZE).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = export::to_text(&profile);
    let path = data(&format!("{name}.profile.txt"));
    if std::env::var_os("EDITOR_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(text, expected, "{name}: its text changed (EDITOR_UPDATE_GOLDEN=1 rewrites it)");
    derive(profile)
}

#[test]
fn the_92_plan_says_total_runtime_and_its_io_times() {
    let r = golden("9.2-total-runtime");
    let p = &r.profile;
    assert_eq!(p.statement.execution_ms, Some(0.061), "`Total runtime` before 9.4");
    assert_eq!(p.statement.planning_ms, None);
    let scan = &p.nodes[0];
    assert_eq!(scan.op, OpKind::IndexScan);
    assert_eq!(scan.index.as_deref(), Some("tenk1_unique1"));
    assert_eq!(scan.scan_direction, Some(ScanDirection::Forward));
    let io = scan.io.unwrap();
    assert_eq!((io.shared_hit, io.read_ms, io.block_size), (Some(3), Some(0.0), Some(8192)));
    assert_eq!(p.depth, ProfileDepth::Actual { timing: true });
}

#[test]
fn a_parallel_plan_keeps_its_workers_and_their_averages() {
    let r = golden("9.6-parallel");
    let p = &r.profile;
    let gather = &p.nodes[1];
    assert_eq!(gather.op, OpKind::Gather);
    let par = gather.parallel.as_ref().unwrap();
    assert_eq!((par.planned, par.launched), (Some(2), Some(2)));
    assert!(gather.extra.iter().any(|e| e.key == "Single Copy"));
    let partial = &p.nodes[2];
    assert!(partial.flags.partial);
    assert_eq!(partial.op, OpKind::Aggregate(AggStrategy::Plain));
    let scan = &p.nodes[3];
    assert!(scan.parallel.as_ref().is_some_and(|x| x.aware && x.per_worker.len() == 2));
    assert_eq!(
        scan.parallel.as_ref().unwrap().per_worker[1].io.and_then(|i| i.shared_hit),
        Some(1488)
    );
    // 37.9 ms × 3 loops under a gather of 3 processes: 37.9 ms, approximate.
    let d = r.derived(NodeId(3));
    assert!((d.inclusive_ms.unwrap() - 37.906).abs() < 1e-6 && d.approx);
    assert_eq!(r.hot_path, [NodeId(0), NodeId(1), NodeId(2), NodeId(3)]);
}

#[test]
fn an_estimated_10_plan_has_its_planning_time() {
    let r = golden("10-summary");
    assert_eq!(r.profile.statement.planning_ms, Some(0.214));
    assert_eq!(r.profile.depth, ProfileDepth::Estimated);
    assert_eq!(r.totals.measure, Some(database::plan::Measure::Cost));
    assert!(r.profile.nodes[0].extra.iter().any(|e| e.key == "Inner Unique"));
    assert_eq!(r.profile.nodes[0].join, Some(JoinKind::Inner));
}

#[test]
fn jit_and_settings_are_the_statements() {
    let r = golden("11-jit");
    let jit = r.profile.statement.jit.unwrap();
    assert_eq!(jit.functions, Some(4));
    assert_eq!(jit.inlining_ms, None, "inlining was off");
    assert_eq!((jit.emission_ms, jit.total_ms), (Some(5.712), Some(6.751)));
    assert!(
        r.profile
            .statement
            .extra
            .iter()
            .any(|e| e.key == "Options" && e.group.as_deref() == Some("JIT"))
    );
    let r = golden("12-settings");
    let names: Vec<&str> = r.profile.statement.settings.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["enable_seqscan", "work_mem"]);
    let bitmap = &r.profile.nodes[0];
    assert_eq!((bitmap.actuals.exact_blocks, bitmap.actuals.lossy_blocks), (Some(90.0), Some(0.0)));
    let index = &r.profile.nodes[1];
    assert_eq!(index.op, OpKind::BitmapIndexScan);
    assert_eq!(
        index.object.as_ref().map(|o| (o.kind, o.name.as_str())),
        Some((ObjectKind::Index, "tenk1_unique1"))
    );
    assert!(export::to_text(&r.profile).contains("->  Bitmap Index Scan on tenk1_unique1"));
}

#[test]
fn a_13_update_has_its_wal_planning_buffers_and_triggers() {
    let r = golden("13-wal-update");
    let p = &r.profile;
    assert_eq!(p.nodes[0].op, OpKind::ModifyTable(WriteKind::Update));
    let wal = p.nodes[0].wal.unwrap();
    assert_eq!((wal.records, wal.bytes), (Some(1), Some(68)));
    assert_eq!(p.statement.planning_io.and_then(|i| i.shared_hit), Some(12));
    assert_eq!(p.statement.triggers[0].name, "accounts_audit");
    assert_eq!(p.statement.triggers[0].calls, Some(1));
    assert_eq!(p.nodes[1].role, Some(ChildRole::Member));
}

#[test]
fn sorts_hashes_and_aggregates_on_disk_are_spills() {
    let r = golden("13-incremental-sort");
    let sort = &r.profile.nodes[1];
    assert_eq!(sort.op, OpKind::IncrementalSort);
    assert_eq!(sort.condition(&CondKind::Presorted), Some("four"));
    assert_eq!(sort.condition(&CondKind::SortKey), Some("four, ten"));
    assert!(
        sort.extra
            .iter()
            .any(|e| e.key == "Full-sort Groups" && matches!(e.value, ExtraValue::Group(_)))
    );

    let r = golden("13-hashagg-disk");
    let agg = &r.profile.nodes[0];
    assert_eq!(agg.op, OpKind::Aggregate(AggStrategy::Hashed));
    assert_eq!(
        agg.spill,
        Some(Spill {
            kind: SpillKind::HashAgg,
            disk_bytes: Some(66_560 * 1024),
            batches: Some(160)
        })
    );
    assert_eq!(agg.memory.as_ref().and_then(|m| m.peak_bytes), Some(4145 * 1024));
    assert!(agg.extra.iter().any(|e| e.key == "Planned Partitions"));

    let r = golden("sort-external");
    let sort = &r.profile.nodes[0];
    assert_eq!(sort.spill.map(|s| s.kind), Some(SpillKind::Sort));
    assert_eq!(sort.memory.as_ref().and_then(|m| m.sort_space), Some((2696 * 1024, Storage::Disk)));

    let r = golden("hash-batches");
    let hash = &r.profile.nodes[2];
    assert_eq!(
        hash.spill,
        Some(Spill { kind: SpillKind::Hash, disk_bytes: None, batches: Some(16) })
    );
    let stats = hash.memory.as_ref().and_then(|m| m.hash).unwrap();
    assert_eq!((stats.batches, stats.original_batches), (Some(16), Some(8)));
    assert_eq!(r.profile.nodes[0].join, Some(JoinKind::Left));

    let r = golden("16-temp-io");
    let io = r.profile.nodes[0].io.unwrap();
    assert_eq!((io.temp_read_ms, io.temp_write_ms), (Some(8.912), Some(21.37)));
    assert_eq!(r.profile.statement.io_total.and_then(|i| i.shared_hit), Some(8334 * 2));
}

#[test]
fn memoize_its_cache_and_the_query_identifier() {
    let r = golden("14-memoize-queryid");
    let p = &r.profile;
    assert_eq!(p.statement.query_id, Some(-2_151_370_651_563_219_536));
    let memo = &p.nodes[2];
    assert_eq!(memo.op, OpKind::Memoize);
    let cache = memo.memory.as_ref().and_then(|m| m.cache).unwrap();
    assert_eq!((cache.hits, cache.misses), (Some(9980), Some(20)));
    assert_eq!(memo.condition(&CondKind::CacheKey), Some("t1.twenty"));
    assert_eq!(p.nodes[0].output, ["t1.unique1", "t2.unique2"]);
    assert_eq!(p.nodes[1].object.as_ref().and_then(|o| o.schema.as_deref()), Some("public"));
}

#[test]
fn a_17_plan_has_its_planning_memory_and_serialization() {
    let r = golden("17-memory-serialize");
    let s = &r.profile.statement;
    assert_eq!(s.planning_memory.and_then(|m| m.used_bytes), Some(23 * 1024));
    let ser = s.serialize.as_ref().unwrap();
    assert_eq!(
        (ser.ms, ser.bytes, ser.format.as_deref()),
        (Some(0.412), Some(58 * 1024), Some("text"))
    );
    assert!(
        s.extra
            .iter()
            .any(|e| e.key == "Shared Hit Blocks" && e.group.as_deref() == Some("Serialization"))
    );
    let io = r.profile.nodes[0].io.unwrap();
    assert_eq!(
        (io.read_ms, io.local_read_ms),
        (Some(0.0), Some(0.0)),
        "17's names of the I/O times"
    );
}

#[test]
fn an_18_plan_has_index_searches_disabled_steps_and_fractional_rows() {
    let r = golden("18-index-searches-disabled");
    let p = &r.profile;
    assert!(p.nodes[1].flags.disabled && !p.nodes[0].flags.disabled);
    assert_eq!(p.nodes[2].actuals.index_searches, Some(50.0));
    assert_eq!(p.nodes[0].estimates.rows, Some(48.5));
    assert_eq!(p.nodes[1].actuals.rows_removed_by_filter, Some(99_950.0));
    // Before 18: a cost past `disable_cost` is a disabled step (only the one it starts at).
    let old = r#"[{"Plan": {"Node Type": "Sort", "Total Cost": 10000000100.0, "Plans": [
        {"Node Type": "Seq Scan", "Relation Name": "t", "Total Cost": 10000000050.0}]}}]"#;
    let p = parse(old, BLOCK_SIZE).unwrap();
    assert!(!p.nodes[0].flags.disabled && p.nodes[1].flags.disabled);
}

#[test]
fn subplans_initplans_and_branches_never_run() {
    let r = golden("nested-loop-subplan");
    let p = &r.profile;
    let sub = &p.nodes[1];
    assert_eq!(
        (sub.role, sub.subplan_name.as_deref()),
        (Some(ChildRole::SubPlan), Some("SubPlan 1"))
    );
    assert_eq!(p.nodes[3].role, Some(ChildRole::InitPlan));
    assert!(p.nodes[4].actuals.never_executed);
    // The subplan ran 1000 times: 58 ms of the scan's 61.
    let d = r.derived(NodeId(1));
    assert!((d.inclusive_ms.unwrap() - 58.0).abs() < 1e-6);
    assert_eq!(p.nodes[2].actuals.heap_fetches, Some(1200.0));
}

#[test]
fn a_materialized_cte_is_paid_once() {
    let r = golden("cte");
    let p = &r.profile;
    assert_eq!(p.nodes[1].subplan_name.as_deref(), Some("CTE totals"));
    let reader = &p.nodes[3];
    assert_eq!(reader.op, OpKind::CteScan);
    assert_eq!(
        reader.object.as_ref().map(|o| (o.kind, o.name.as_str())),
        Some((ObjectKind::Cte, "totals"))
    );
    // The join (3.402) minus its readers (2.741, the hash's 0.081): the CTE's plan isn't taken
    // out of it again.
    let join = r.derived(NodeId(0)).self_ms.unwrap();
    assert!((join - (3.402 - 2.741 - 0.081)).abs() < 1e-6, "{join}");
    let first = r.derived(NodeId(3)).self_ms.unwrap();
    assert!((first - (2.741 - 2.688)).abs() < 1e-6, "{first}");
}

/// Every key of the answer has its place, or is kept as an extra with its type: nothing is
/// lost — a newer version's keys show without new code.
#[test]
fn unknown_keys_are_kept_with_their_types() {
    let json = std::fs::read_to_string(data("unknown-keys.json")).unwrap();
    let r = golden("unknown-keys");
    let p = &r.profile;
    let root: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Every node key: known, or an extra of its node.
    fn walk(v: &serde_json::Value, p: &Profile, at: &mut usize) {
        let node = &p.nodes[*at];
        *at += 1;
        for (key, value) in v.as_object().unwrap() {
            if KEYS.contains(&key.as_str()) {
                continue;
            }
            let found = node.extra.iter().find(|e| e.key == *key);
            assert!(found.is_some(), "{key} lost");
            assert_eq!(found.unwrap().value, extra_value(value));
        }
        if let Some(serde_json::Value::Array(children)) = v.get("Plans") {
            for c in children {
                walk(c, p, at);
            }
        }
    }
    let mut at = 0;
    walk(&root[0]["Plan"], p, &mut at);
    let teleport = &p.nodes[0];
    assert_eq!(teleport.op, OpKind::Other, "a node type not known: its words kept");
    let value = |key: &str| teleport.extra.iter().find(|e| e.key == key).map(|e| e.value.clone());
    assert_eq!(value("Quantum Flux"), Some(ExtraValue::Float(1.21)));
    assert_eq!(value("Flux Capacitors"), Some(ExtraValue::Int(3)));
    assert_eq!(value("Flux Enabled"), Some(ExtraValue::Bool(true)));
    assert!(matches!(value("Flux Targets"), Some(ExtraValue::List(l)) if l.len() == 3));
    assert!(
        matches!(value("Flux Detail"), Some(ExtraValue::Group(g)) if g.iter().any(|e| matches!(e.value, ExtraValue::Group(_))))
    );
    let grouped = teleport.extra.iter().find(|e| e.key == "Worker Affinity").unwrap();
    assert_eq!(grouped.group.as_deref(), Some("Workers"));
    // A worker's key, the statement's, the JIT's, a trigger's, the planning's.
    let worker = &p.nodes[1].parallel.as_ref().unwrap().per_worker[0];
    assert!(worker.extra.iter().any(|e| e.key == "Worker Mood"));
    let s = &p.statement;
    let has = |key: &str, group: Option<&str>| {
        s.extra.iter().any(|e| e.key == key && e.group.as_deref() == group)
    };
    assert!(has("Statement Mood", None));
    assert!(has("Warp", Some("JIT")) && has("Compiler", Some("JIT")));
    assert!(has("Constraint Name", Some("Triggers")));
    assert!(has("Planner Cache", Some("Planning")));
    let jit = s.jit.unwrap();
    assert_eq!(
        (jit.generation_ms, jit.deform_ms, jit.inlining_ms),
        (Some(0.5), Some(0.1), Some(0.0))
    );
    assert!(STATEMENT_KEYS.contains(&"Serialization"));
}

/// A plan of 5 000 partitions read well under 50 ms (release; generous in debug).
#[test]
fn a_plan_of_five_thousand_nodes_is_read_within_its_budget() {
    let mut parts = Vec::new();
    for i in 0..4_999 {
        parts.push(format!(
            r#"{{"Node Type": "Seq Scan", "Parent Relationship": "Member", "Relation Name": "events_{i}", "Alias": "events_{i}", "Startup Cost": 0.0, "Total Cost": 35.5, "Plan Rows": 2550, "Plan Width": 4, "Actual Startup Time": 0.002, "Actual Total Time": 0.004, "Actual Rows": 0, "Actual Loops": 1, "Filter": "(kind = 3)", "Rows Removed by Filter": 0, "Shared Hit Blocks": 1}}"#
        ));
    }
    let json = format!(
        r#"[{{"Plan": {{"Node Type": "Append", "Startup Cost": 0.0, "Total Cost": 177462.0, "Plan Rows": 12747450, "Plan Width": 4, "Actual Startup Time": 0.01, "Actual Total Time": 30.0, "Actual Rows": 0, "Actual Loops": 1, "Plans": [{}]}}, "Planning Time": 80.0, "Execution Time": 31.0}}]"#,
        parts.join(",")
    );
    let started = std::time::Instant::now();
    let p = parse(&json, BLOCK_SIZE).unwrap();
    let took = started.elapsed();
    assert_eq!(p.nodes.len(), 5_000);
    let budget = if cfg!(debug_assertions) { 1_000 } else { 50 };
    assert!(took.as_millis() < budget, "{took:?}");
    let r = derive(p);
    assert_eq!(r.hot_path.len(), 2);
}

// ─── The analysis (39h3): each fixture's findings ───

/// What the rules find in fixture `name` profiled from `sql` (nothing known of its tables).
fn findings(name: &str, sql: &str) -> Vec<database::profile::rules::Finding> {
    use database::console::PlannedStatement;
    use database::profile::rules::{Facts, RecentRuns, RuleContext, Thresholds, analyze};
    let json = std::fs::read_to_string(data(&format!("{name}.json"))).unwrap();
    let mut profile = parse(&json, BLOCK_SIZE).unwrap();
    profile.sum_io();
    let report = derive(profile);
    let flavor = database::sql::Flavor::Postgres { version: Some(170_000) };
    let tokens = database::sql::lex(sql, flavor);
    let statement = PlannedStatement {
        text: sql.to_owned(),
        range: 0..sql.len(),
        kind: database::sql::classify(sql, &tokens, flavor),
        bound: None,
    };
    let (facts, recent, thresholds) =
        (Facts::default(), RecentRuns::default(), Thresholds::DEFAULT);
    let cx = RuleContext::new(&report, &facts, &statement, flavor, &recent, &thresholds, false);
    analyze(&cx)
}

fn rules_of(found: &[database::profile::rules::Finding]) -> Vec<&'static str> {
    found.iter().map(|f| f.rule).collect()
}

#[test]
fn each_fixture_is_analyzed_into_what_it_shows() {
    use database::profile::rules::Severity;
    // A disabled sequential scan keeping 50 of 100 000 rows.
    let sql = "SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id \
               WHERE o.status IN ('new', 'paid')";
    let found = findings("18-index-searches-disabled", sql);
    assert_eq!(rules_of(&found)[0], "hot-spot");
    let scan = found.iter().find(|f| f.rule == "seq-scan-selective").expect("the selective scan");
    assert_eq!(scan.severity, Severity::Critical, "90 % of the time");
    assert_eq!(&sql[scan.sql_range.clone().unwrap()], "o.status IN ('new', 'paid')");
    let disabled = found.iter().find(|f| f.rule == "disabled-node").unwrap();
    assert_eq!(
        disabled.suggestion.as_ref().and_then(|s| s.sql()),
        Some("SET enable_seqscan = on;")
    );
    // Sorts and hashes on disk.
    let sql = "SELECT * FROM tenk1 ORDER BY stringu1 DESC";
    let found = findings("sort-external", sql);
    let spill = found.iter().find(|f| f.rule == "spill-to-disk").unwrap();
    assert_eq!(spill.suggestion.as_ref().and_then(|s| s.sql()), Some("SET work_mem = '8MB';"));
    let found = findings("16-temp-io", "SELECT * FROM blobs ORDER BY payload");
    assert!(rules_of(&found).contains(&"spill-to-disk"));
    let found = findings("13-hashagg-disk", "SELECT customer_id, count(*) FROM orders GROUP BY 1");
    let spill = found.iter().find(|f| f.rule == "spill-to-disk").unwrap();
    assert_eq!(spill.text_of(database::profile::rules::EvidenceKey::Spill), Some("hash_agg"));
    let found = findings(
        "hash-batches",
        "SELECT * FROM orders o LEFT JOIN customers c ON c.id = o.customer_id",
    );
    assert!(rules_of(&found).contains(&"spill-to-disk"));
    // An index-only scan still visiting the table.
    let found = findings(
        "nested-loop-subplan",
        "SELECT * FROM customers c WHERE (SELECT count(*) FROM orders o WHERE o.customer_id = c.id) > 2",
    );
    let heap = found.iter().find(|f| f.rule == "heap-fetches").unwrap();
    assert_eq!(
        heap.suggestion.as_ref().and_then(|s| s.sql()),
        Some("VACUUM (ANALYZE) \"orders\";")
    );
    // Planner settings changed by hand.
    let found = findings("12-settings", "SELECT * FROM tenk1 WHERE unique1 < 100");
    let setting = found.iter().find(|f| f.rule == "disabled-node").unwrap();
    assert_eq!(setting.variant, Some("setting"));
    assert!(!rules_of(&found).contains(&"lossy-bitmap"), "no lossy block");
}

#[test]
fn healthy_fixtures_have_nothing_to_say() {
    // JIT's 7 ms of 146, both workers launched, a quick index lookup, a CTE paid once.
    for (name, sql) in [
        ("11-jit", "SELECT count(*) FROM events WHERE kind > 0"),
        ("9.6-parallel", "SELECT count(*) FROM pgbench_accounts"),
        ("9.2-total-runtime", "SELECT * FROM tenk1 WHERE unique1 = 42"),
        (
            "14-memoize-queryid",
            "SELECT t1.unique1, t2.unique2 FROM tenk1 t1 JOIN tenk1 t2 ON t2.unique2 = t1.twenty",
        ),
        ("13-incremental-sort", "SELECT * FROM tenk1 ORDER BY four, ten LIMIT 10"),
        ("17-memory-serialize", "SELECT * FROM customers"),
    ] {
        let found = findings(name, sql);
        let problems: Vec<&str> = found.iter().filter(|f| f.is_problem()).map(|f| f.rule).collect();
        assert!(problems.is_empty(), "{name}: {problems:?}");
    }
}
