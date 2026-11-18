use super::testing::{ProfileBuilder, planned, ran};
use super::*;
use crate::capabilities::ExplainKinds;

fn plan(op: &str) -> PlanNode {
    PlanNode::new(op)
}

#[test]
fn a_plan_becomes_a_profile_and_back_unchanged() {
    let mut join = PlanNode {
        object: None,
        detail: Some("Hash Cond: (o.customer_id = c.id)".into()),
        startup_cost: Some(1.07),
        total_cost: Some(2.21),
        plan_rows: Some(5.0),
        actual_rows: Some(5.0),
        actual_time_ms: Some(0.04),
        loops: Some(1.0),
        ..plan("Hash Join (Left)")
    };
    join.children.push(PlanNode {
        object: Some("orders o".into()),
        detail: Some("Filter: (total > 0)".into()),
        ..plan("Seq Scan")
    });
    join.children.push(PlanNode { loops: Some(0.0), ..plan("Hash") });
    let roots = vec![join, plan("Result")];
    let profile =
        Profile::from_plan(&roots, ProfileDepth::Actual { timing: true }, Engine::default());
    assert_eq!(profile.roots.len(), 2);
    assert_eq!(profile.nodes.len(), 4);
    assert_eq!(profile.node(NodeId(0)).op, OpKind::HashJoin, "its kind guessed from its words");
    assert_eq!(profile.node(NodeId(1)).op, OpKind::SeqScan);
    assert_eq!(profile.node(NodeId(1)).parent, Some(NodeId(0)));
    assert!(profile.node(NodeId(2)).actuals.never_executed, "no loop: never executed");
    assert!(profile.collected.actual && profile.collected.node_timing);
    assert_eq!(profile.plan_nodes(), roots, "the projection gives the plan back");
    assert_eq!(profile.format, PROFILE_FORMAT);
}

#[test]
fn the_projection_says_what_the_words_dont() {
    // PostgreSQL's: the join's kind, the index used, the conditions with their labels.
    let p = ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .child(OpKind::HashJoin, "Hash Join", |n| {
            n.join = Some(JoinKind::Left);
            n.conditions.push(Condition::new(CondKind::HashCond, "(a = b)"));
        })
        .node(OpKind::IndexScan, "Index Scan", |n| {
            n.object = Some(ObjectRef {
                alias: Some("c".into()),
                ..ObjectRef::new(ObjectKind::Table, "customers")
            });
            n.index = Some("customers_pkey".into());
            n.conditions.push(Condition::new(CondKind::IndexCond, "(id > 0)"));
        })
        .node(OpKind::Sort, "Sort", |n| {
            n.conditions.push(Condition::new(CondKind::SortKey, "a, b DESC"));
            n.conditions.push(Condition::new(CondKind::Other("Order By".into()), "x <-> y"));
        })
        .build();
    let nodes = p.plan_nodes();
    assert_eq!(nodes[0].operation, "Hash Join (Left)");
    assert_eq!(nodes[0].detail.as_deref(), Some("Hash Cond: (a = b)"));
    let index = &nodes[0].children[0];
    assert_eq!(index.object.as_deref(), Some("customers c"));
    assert_eq!(index.detail.as_deref(), Some("using customers_pkey; Index Cond: (id > 0)"));
    let sort = &nodes[0].children[1];
    assert_eq!(sort.detail.as_deref(), Some("Sort Key: a, b DESC; Order By: x <-> y"));
    // SQLite's: its words already hold the index and the condition, said once.
    let s = ProfileBuilder::new("sqlite", ProfileDepth::Estimated)
        .node(OpKind::IndexScan, "SEARCH users USING INDEX users_name (name=?)", |n| {
            n.object = Some(ObjectRef::new(ObjectKind::Table, "users"));
            n.index = Some("users_name".into());
            n.conditions.push(Condition::new(CondKind::IndexCond, "name=?"));
        })
        .build();
    let nodes = s.plan_nodes();
    assert_eq!(nodes[0].object.as_deref(), Some("users"));
    assert_eq!(nodes[0].detail, None, "nothing its words don't say");
}

#[test]
fn a_tree_is_walked_depth_first_and_a_broken_list_once() {
    let p = ProfileBuilder::new("t", ProfileDepth::Estimated)
        .child(OpKind::Limit, "Limit", |_| {})
        .child(OpKind::Sort, "Sort", |_| {})
        .node(OpKind::SeqScan, "Seq Scan", |_| {})
        .up()
        .node(OpKind::Result, "Result", |_| {})
        .up()
        .node(OpKind::Result, "Second root", |_| {})
        .build();
    let walked: Vec<(u32, usize)> = p.walk().iter().map(|(id, d)| (id.0, *d)).collect();
    assert_eq!(walked, [(0, 0), (1, 1), (2, 2), (3, 1), (4, 0)]);
    let mut broken = p;
    // A child listed twice and a child that isn't there.
    broken.nodes[0].children.push(NodeId(2));
    broken.nodes[0].children.push(NodeId(99));
    assert_eq!(broken.walk().len(), 5);
    assert_eq!(broken.plan_nodes().iter().map(|n| n.walk().len()).sum::<usize>(), 5);
    // A parent that isn't there yet: a root.
    let mut q = Profile::new(Engine::default(), ProfileDepth::Estimated);
    let id = q.push(Some(NodeId(7)), ProfileNode::new(OpKind::Other, "x"));
    assert_eq!(q.roots, [id]);
}

#[test]
fn the_engines_answer_is_kept_within_its_limit() {
    let mut p = Profile::new(Engine::default(), ProfileDepth::Estimated);
    p.keep_raw(RawFormat::Json, "[1]", 10);
    assert_eq!(p.raw.as_ref().map(|r| &*r.text), Some("[1]"));
    p.keep_raw(RawFormat::Json, &"x".repeat(11), 10);
    assert_eq!(p.raw, None);
    assert!(p.has_note(ProfileNote::RawTooLarge { bytes: 11 }));
    // Noted once.
    p.note(ProfileNote::RolledBack);
    p.note(ProfileNote::RolledBack);
    assert_eq!(p.notes.iter().filter(|n| **n == ProfileNote::RolledBack).count(), 1);
    for note in [
        ProfileNote::RolledBack,
        ProfileNote::IoTimingOff,
        ProfileNote::InstrumentationOverhead,
        ProfileNote::PlannerSettingsChanged,
        ProfileNote::NewerFormat { format: 3 },
        ProfileNote::Truncated { nodes: 60_000 },
        ProfileNote::RawTooLarge { bytes: 9 },
        ProfileNote::NoStatementStats,
    ] {
        assert_eq!(ProfileNote::from_id(note.id(), note.argument()), Some(note));
    }
}

#[test]
fn a_large_plan_is_noted_and_io_is_added_up() {
    let mut p = Profile::new(Engine::default(), ProfileDepth::Estimated);
    for _ in 0..=LARGE_PLAN {
        p.push(None, ProfileNode::new(OpKind::Result, "Result"));
    }
    p.mark_size();
    assert!(p.has_note(ProfileNote::Truncated { nodes: LARGE_PLAN + 1 }));
    let mut q = ProfileBuilder::new("t", ProfileDepth::Actual { timing: true })
        .child(OpKind::HashJoin, "Hash Join", |n| {
            n.io = Some(Io { shared_hit: Some(2), block_size: Some(8192), ..Io::default() });
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            n.io = Some(Io { shared_hit: Some(3), shared_read: Some(4), ..Io::default() });
        })
        .node(OpKind::Hash, "Hash", |_| {})
        .build();
    q.sum_io();
    let total = q.statement.io_total.unwrap();
    assert_eq!((total.shared_hit, total.shared_read, total.temp_read), (Some(5), Some(4), None));
    assert_eq!(total.bytes(2), 16384);
    assert!(q.memory() > 0);
}

#[test]
fn the_kinds_are_guessed_from_an_engines_words() {
    let cases = [
        ("Seq Scan", OpKind::SeqScan),
        ("Parallel Seq Scan", OpKind::SeqScan),
        ("Index Only Scan", OpKind::IndexOnlyScan),
        ("Bitmap Heap Scan", OpKind::BitmapHeapScan),
        ("Hash Join (Left)", OpKind::HashJoin),
        ("HashAggregate", OpKind::Aggregate(AggStrategy::Hashed)),
        ("GroupAggregate", OpKind::Aggregate(AggStrategy::Sorted)),
        ("Aggregate", OpKind::Aggregate(AggStrategy::Plain)),
        ("Gather Merge", OpKind::GatherMerge),
        ("Toy Scan", OpKind::Other),
        ("Insert", OpKind::ModifyTable(WriteKind::Insert)),
        ("Table scan on t", OpKind::SeqScan),
    ];
    for (words, kind) in cases {
        assert_eq!(OpKind::guess(words), kind, "{words}");
    }
    for kind in OpKind::ALL {
        assert_eq!(OpKind::from_id(kind.id()), Some(kind));
    }
    for unit in Unit::ALL {
        assert_eq!(Unit::from_id(unit.id()), Some(unit));
    }
    for j in JoinKind::ALL {
        assert_eq!(JoinKind::from_words(j.words()), Some(j));
        assert_eq!(JoinKind::from_id(j.id()), Some(j));
    }
}

#[test]
fn what_cant_run_in_a_transaction_isnt_profiled_as_it_runs() {
    let pg = Flavor::Postgres { version: Some(170_000) };
    for (text, words) in [
        ("VACUUM ANALYZE t", "VACUUM"),
        ("create index concurrently i on t (a)", "CREATE INDEX CONCURRENTLY"),
        ("CREATE UNIQUE INDEX CONCURRENTLY i ON t (a)", "CREATE UNIQUE INDEX CONCURRENTLY"),
        ("CREATE DATABASE x", "CREATE DATABASE"),
        ("ALTER SYSTEM SET work_mem = '1GB'", "ALTER SYSTEM"),
        ("REINDEX TABLE CONCURRENTLY t", "REINDEX CONCURRENTLY"),
        ("COMMIT", "COMMIT"),
        ("-- a comment\nBEGIN", "BEGIN"),
    ] {
        assert_eq!(actual_refused(text, pg).as_deref(), Some(words), "{text}");
    }
    for text in ["SELECT 1", "UPDATE t SET a = 1", "CREATE INDEX i ON t (a)", "REINDEX TABLE t"] {
        assert_eq!(actual_refused(text, pg), None, "{text}");
    }
    assert_eq!(actual_refused("VACUUM", Flavor::Sqlite).as_deref(), Some("VACUUM"));
    assert_eq!(actual_refused("ATTACH 'x' AS y", Flavor::Sqlite).as_deref(), Some("ATTACH"));
}

#[test]
fn a_session_that_only_explains_profiles_through_its_plan() {
    struct Explaining;
    impl Session for Explaining {
        fn server(&self) -> String {
            "Old 1.0".into()
        }
        fn children(&mut self, _: &[String]) -> Result<Vec<crate::Node>, DbError> {
            Ok(Vec::new())
        }
        fn execute(
            &mut self,
            _: &str,
            _: &mut dyn crate::RowSink,
            _: &Cancel,
        ) -> Result<crate::Outcome, DbError> {
            Ok(crate::Outcome::default())
        }
        fn explain(
            &mut self,
            _: &str,
            analyze: bool,
            _: &Cancel,
        ) -> Result<Vec<PlanNode>, DbError> {
            Ok(vec![PlanNode {
                total_cost: Some(3.0),
                actual_rows: analyze.then_some(3.0),
                ..PlanNode::new("Seq Scan")
            }])
        }
    }
    let mut session = Explaining;
    let estimated =
        session.profile("SELECT 1", &ProfileOptions::ESTIMATED, &Cancel::default()).unwrap();
    assert_eq!(estimated.engine.name, "Old 1.0");
    assert_eq!(estimated.depth, ProfileDepth::Estimated);
    assert_eq!(estimated.nodes[0].op, OpKind::SeqScan);
    let actual = session.profile("SELECT 1", &ProfileOptions::ACTUAL, &Cancel::default()).unwrap();
    assert_eq!(actual.nodes[0].actuals.rows, Some(3.0));
    assert!(matches!(
        session.table_stats(&[crate::TableRef::new("t")], &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
    // What a plan-only driver's profiles hold.
    let caps = ProfileCaps::from_explain(ExplainKinds { plan: true, analyze: false });
    assert!(caps.estimated && !caps.actual && !caps.buffers);
    assert!(!ProfileCaps::NONE.any());
    let mut set = ProfileCaps::NONE;
    for (name, _) in ProfileCaps::NONE.fields() {
        set.set(name, true);
    }
    assert!(set.fields().iter().all(|(_, on)| *on), "every field named");
}

/// The model holds MySQL's and MariaDB's profiles as their drivers will give them (plan
/// 39h2: no parser yet, the model checked on their shape): MySQL's `EXPLAIN ANALYZE` tree,
/// its session status differences as counters, `performance_schema`'s waits; MariaDB's
/// `ANALYZE FORMAT=JSON` with its `r_*` figures.
#[test]
fn mysqls_and_mariadbs_profiles_fit_the_model() {
    let mysql = ProfileBuilder::new("mysql", ProfileDepth::Actual { timing: true })
        .engine("MySQL", Some("8.4.2"))
        .child(OpKind::NestedLoop, "Nested loop inner join", |n| {
            n.join = Some(JoinKind::Inner);
            ran(n, 0.1, 12.5, 120.0, 1.0);
            planned(n, 0.0, 412.0, 98.0);
        })
        .node(OpKind::SeqScan, "Table scan on o", |n| {
            n.object = Some(ObjectRef {
                alias: Some("o".into()),
                ..ObjectRef::new(ObjectKind::Table, "orders")
            });
            n.conditions.push(Condition::new(CondKind::Filter, "(o.total > 10)"));
            ran(n, 0.05, 8.0, 1000.0, 1.0);
            n.estimates.rows = Some(990.0);
            n.extra.push(Extra::new(
                "filtered",
                ExtraValue::Quantity(Quantity::new(33.3, Unit::Percent)),
            ));
            n.extra
                .push(Extra::new("cost_info.read_cost", ExtraValue::Float(12.5)).in_group("Cost"));
        })
        .node(OpKind::IndexScan, "Single-row index lookup on c using PRIMARY", |n| {
            n.object = Some(ObjectRef::new(ObjectKind::Table, "customers"));
            n.index = Some("PRIMARY".into());
            n.conditions.push(Condition::new(CondKind::IndexCond, "(c.id = o.customer_id)"));
            ran(n, 0.001, 0.002, 1.0, 120.0);
            n.extra.push(Extra::new("access_type", ExtraValue::Text("eq_ref".into())));
            n.extra.push(Extra::new(
                "used_key_parts",
                ExtraValue::List(vec![ExtraValue::Text("id".into())]),
            ));
        })
        .with(|p| {
            p.statement.execution_ms = Some(12.6);
            p.statement.counters = Some(EngineCounters {
                rows_examined: Some(1120),
                select_scan: Some(1),
                sort_merge_passes: Some(0),
                tmp_disk_tables: Some(0),
                ..EngineCounters::default()
            });
            p.statement.waits.push(Wait {
                event: "wait/io/table/sql/handler".into(),
                ms: Some(3.2),
                count: Some(1120),
            });
            p.statement.extra.push(Extra::new("NO_INDEX_USED", ExtraValue::Bool(true)));
        })
        .build();
    let report = derive(mysql.clone());
    assert_eq!(report.totals.time_ms, Some(12.6));
    // The lookup ran 120 times: 0.24 ms in all.
    let lookup = report.derived(NodeId(2));
    assert!((lookup.inclusive_ms.unwrap() - 0.24).abs() < 1e-9);
    let json = export::to_json(&mysql);
    for key in ["\"rows_examined\": 1120", "wait/io/table/sql/handler", "\"unit\": \"percent\""] {
        assert!(json.contains(key), "{key} in {json}");
    }

    let mariadb = ProfileBuilder::new("mariadb", ProfileDepth::Actual { timing: true })
        .engine("MariaDB", Some("11.4"))
        .node(OpKind::SeqScan, "ALL", |n| {
            n.object = Some(ObjectRef::new(ObjectKind::Table, "t"));
            n.estimates.rows = Some(1000.0);
            n.actuals.loops = Some(1.0);
            n.actuals.rows = Some(1000.0);
            n.actuals.total_ms = Some(0.9);
            n.extra.push(Extra::new(
                "r_filtered",
                ExtraValue::Quantity(Quantity::new(10.0, Unit::Percent)),
            ));
            n.extra.push(Extra::new(
                "r_table_time_ms",
                ExtraValue::Quantity(Quantity::new(0.7, Unit::Millis)),
            ));
            n.extra.push(Extra::new(
                "r_other_time_ms",
                ExtraValue::Quantity(Quantity::new(0.2, Unit::Millis)),
            ));
        })
        .build();
    let text = export::to_text(&mariadb);
    assert!(text.contains("ALL on t"), "{text}");
    assert!(text.contains("r_filtered: 10 percent"), "{text}");
}
