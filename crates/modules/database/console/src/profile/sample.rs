//! The snapshots' profile (the `snapshot` feature, and the tests): a PostgreSQL 17 profile built by
//! hand — a parallel plan of a month's pending orders by customer, its workers short of
//! those planned, its sort spilling to disk, its orders read whole to keep a few, an
//! estimate far off — analyzed by the real rules with what's known of its tables, shown as
//! a console's ([`show`]). No server: its figures are made up, its shape is PostgreSQL's.
//!
//! For 39h6: the same query after an index on `orders (status, created_at)` ([`indexed`]:
//! an index scan and a nested loop, 12 ms instead of 845) to compare with ([`show_compare`]),
//! and the tables' statistics as PostgreSQL would give them ([`show_tables`]). And what the
//! snapshots freeze of a real profile ([`freeze_times`]: the engine's own times).

use std::sync::Arc;

use database::console::PlannedStatement;
use database::profile::node::{
    AggStrategy, CondKind, Io, Memory, OpKind, Parallel, Spill, SpillKind, Storage,
};
use database::profile::rules::{Facts, RecentRuns, RuleContext, Thresholds};
use database::profile::statement::Jit;
use database::profile::testing::{ProfileBuilder, cond, on, planned, ran, table};
use database::profile::{Extra, ExtraValue, ProfileDepth, ProfileReport, Quantity, Unit};
use database::sql::{self, Flavor};
use ui_components::ServiceId;

use crate::runner::{ProfileShown, Runner};

/// The statement profiled (the sample console's text).
pub const SQL: &str = "-- This month's pending orders, by customer\n\
SELECT c.name, count(*) AS orders, sum(o.total) AS revenue\n\
FROM orders o\n\
JOIN customers c ON c.id = o.customer_id\n\
WHERE o.status = 'pending'\n  AND o.created_at >= now() - interval '30 days'\n\
GROUP BY c.name\n\
ORDER BY revenue DESC\n\
LIMIT 50;\n";

const FLAVOR: Flavor = Flavor::Postgres { version: Some(170_002) };

fn io(hit: u64, read: u64) -> Io {
    Io { shared_hit: Some(hit), shared_read: Some(read), block_size: Some(8192), ..Io::default() }
}

/// The sample's report, its findings in: `text` the console's (the statement in it).
pub fn report(text: &str) -> Arc<ProfileReport> {
    report_with(text, false).0
}

/// The sample's report with the facts its rules knew (`stats`: the tables' statistics too).
pub fn report_with(text: &str, stats: bool) -> (Arc<ProfileReport>, Facts) {
    let start = text.find("SELECT").unwrap_or(0);
    let end = text.rfind(';').map_or(text.len(), |e| e + 1);
    let statement = &text[start..end];
    let profile = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .engine("PostgreSQL", Some("17.2"))
        .child(OpKind::Limit, "Limit", |n| {
            planned(n, 128_402.6, 128_402.73, 50.0);
            ran(n, 844.48, 844.52, 50.0, 1.0);
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Sort, "Sort", |n| {
            planned(n, 128_402.6, 128_405.55, 1180.0);
            ran(n, 844.46, 844.47, 50.0, 1.0);
            cond(n, CondKind::SortKey, "(sum(o.total)) DESC");
            n.memory = Some(Memory {
                sort_method: Some("top-N heapsort".into()),
                sort_space: Some((32 * 1024, Storage::Memory)),
                ..Memory::default()
            });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Aggregate(AggStrategy::Sorted), "Finalize GroupAggregate", |n| {
            planned(n, 127_810.1, 128_366.9, 1180.0);
            ran(n, 640.2, 843.9, 1180.0, 1.0);
            cond(n, CondKind::GroupKey, "c.name");
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::GatherMerge, "Gather Merge", |n| {
            planned(n, 127_810.1, 128_331.5, 4720.0);
            ran(n, 640.1, 841.2, 3410.0, 1.0);
            n.parallel = Some(Parallel { planned: Some(4), launched: Some(2), ..Parallel::default() });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Sort, "Sort", |n| {
            planned(n, 126_810.0, 126_812.9, 1180.0);
            ran(n, 600.4, 781.3, 1137.0, 3.0);
            cond(n, CondKind::SortKey, "c.name");
            n.memory = Some(Memory {
                sort_method: Some("external merge".into()),
                sort_space: Some((51_200 * 1024, Storage::Disk)),
                ..Memory::default()
            });
            n.spill = Some(Spill { kind: SpillKind::Sort, disk_bytes: Some(51_200 * 1024), batches: None });
            n.io = Some(Io { temp_read: Some(6400), temp_written: Some(6400), ..io(0, 0) });
            n.parallel = Some(Parallel { aware: false, ..Parallel::default() });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Aggregate(AggStrategy::Hashed), "Partial HashAggregate", |n| {
            planned(n, 126_700.4, 126_712.2, 1180.0);
            ran(n, 548.7, 560.2, 1137.0, 3.0);
            cond(n, CondKind::GroupKey, "c.name");
            n.memory = Some(Memory { peak_bytes: Some(409_600), ..Memory::default() });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::HashJoin, "Parallel Hash Join", |n| {
            planned(n, 1240.0, 126_640.1, 40.0);
            ran(n, 14.2, 541.4, 6133.0, 3.0);
            cond(n, CondKind::HashCond, "(o.customer_id = c.id)");
            n.parallel = Some(Parallel { aware: true, ..Parallel::default() });
            n.estimates.width = Some(26.0);
        })
        .node(OpKind::SeqScan, "Parallel Seq Scan", |n| {
            on(n, "orders", Some("o"));
            if let Some(o) = n.object.as_mut() {
                o.schema = Some("public".into());
            }
            planned(n, 0.0, 125_380.0, 40.0);
            ran(n, 0.41, 505.3, 6133.0, 3.0);
            cond(
                n,
                CondKind::Filter,
                "(((status)::text = 'pending'::text) AND (created_at >= (now() - '30 days'::interval)))",
            );
            n.actuals.rows_removed_by_filter = Some(401_444.0);
            n.io = Some(io(12_400, 41_800));
            n.parallel = Some(Parallel { aware: true, ..Parallel::default() });
            n.estimates.width = Some(22.0);
            n.extra.push(Extra::new("Async Capable", ExtraValue::Bool(false)));
        })
        .child(OpKind::Hash, "Parallel Hash", |n| {
            planned(n, 980.0, 980.0, 4000.0);
            ran(n, 12.6, 12.6, 4000.0, 3.0);
            n.memory = Some(Memory {
                peak_bytes: Some(1_020 * 1024),
                hash: Some(database::profile::node::HashStats {
                    buckets: Some(16_384),
                    original_buckets: Some(16_384),
                    batches: Some(1),
                    original_batches: Some(1),
                }),
                ..Memory::default()
            });
            n.estimates.width = Some(18.0);
        })
        .node(OpKind::SeqScan, "Parallel Seq Scan", |n| {
            on(n, "customers", Some("c"));
            if let Some(o) = n.object.as_mut() {
                o.schema = Some("public".into());
            }
            planned(n, 0.0, 860.0, 4000.0);
            ran(n, 0.02, 6.1, 4000.0, 3.0);
            n.io = Some(io(740, 0));
            n.parallel = Some(Parallel { aware: true, ..Parallel::default() });
            n.estimates.width = Some(18.0);
        })
        .with(|p| {
            p.statement.planning_ms = Some(1.18);
            p.statement.execution_ms = Some(845.06);
            p.statement.wall_ms = Some(851.4);
            p.statement.rows_returned = Some(50);
            p.statement.jit = Some(Jit {
                functions: Some(18),
                generation_ms: Some(2.9),
                inlining_ms: Some(31.4),
                optimization_ms: Some(36.8),
                emission_ms: Some(21.2),
                total_ms: Some(92.3),
                ..Jit::default()
            });
            p.statement.extra.push(
                Extra::new("Query Identifier", ExtraValue::Int(-7_316_924_550_214_830_112)).in_group("Query"),
            );
            p.statement.extra.push(Extra::new(
                "Planning Buffers",
                ExtraValue::Quantity(Quantity::new(96.0, Unit::Blocks)),
            ));
            p.collected.buffers = true;
            p.collected.parallel = true;
            p.collected.jit = true;
            p.collected.spill = true;
        })
        .build();
    let mut profile = profile;
    profile.sum_io();
    let report = database::profile::derive(profile);
    let facts = Facts {
        tables: vec![
            table(
                "public.orders",
                &[
                    "id:integer",
                    "customer_id:integer",
                    "status:character varying",
                    "created_at:timestamp with time zone",
                    "total:numeric",
                ],
                &[("orders_pkey", &["id"])],
            ),
            table(
                "public.customers",
                &["id:integer", "name", "email"],
                &[("customers_pkey", &["id"])],
            ),
        ],
        ..Facts::default()
    };
    let facts = if stats { with_stats(facts) } else { facts };
    analyzed(report, &facts, statement, start..end)
}

/// `report` analyzed by the real rules with `facts` (`statement` the text, at `range`).
fn analyzed(
    report: ProfileReport,
    facts: &Facts,
    statement: &str,
    range: std::ops::Range<usize>,
) -> (Arc<ProfileReport>, Facts) {
    let tokens = sql::lex(statement, FLAVOR);
    let planned = PlannedStatement {
        text: statement.to_owned(),
        range,
        kind: sql::classify(statement, &tokens, FLAVOR),
        bound: None,
    };
    let recent = RecentRuns::default();
    let thresholds = Thresholds::DEFAULT;
    let findings = {
        let cx = RuleContext::new(&report, facts, &planned, FLAVOR, &recent, &thresholds, false);
        database::profile::rules::analyze(&cx)
    };
    (crate::profile::analysis::with_findings(report, findings), facts.clone())
}

/// The sample's tables as PostgreSQL's statistics would show them: rows live and dead,
/// sizes, scans, when they were last analyzed and vacuumed, their indexes (one never used).
fn with_stats(mut facts: Facts) -> Facts {
    use database::profile::{IndexStats, TableStats};
    const MB: u64 = 1024 * 1024;
    for t in &mut facts.tables {
        let mut stats = TableStats::new(t.table.clone());
        if t.table.name == "orders" {
            if let Some(s) = t.structure.as_mut() {
                s.indexes.push(database::IndexInfo::new(
                    "idx_orders_created",
                    vec!["created_at".to_owned()],
                    false,
                ));
            }
            stats.live_rows = Some(412_480);
            stats.dead_rows = Some(18_906);
            stats.table_bytes = Some(64 * MB + 312 * 1024);
            stats.index_bytes = Some(19 * MB + 96 * 1024);
            stats.total_bytes = Some(83 * MB + 408 * 1024);
            stats.seq_scans = Some(1_284);
            stats.idx_scans = Some(9_802_113);
            stats.last_autoanalyze = Some("2026-10-29 03:12:44+00".to_owned());
            stats.last_vacuum = Some("2026-10-28 22:40:05+00".to_owned());
            stats.modified_since_analyze = Some(3_120);
            stats.stats_present = Some(true);
            stats.indexes = vec![
                IndexStats {
                    name: "orders_pkey".into(),
                    columns: vec!["id".into()],
                    unique: true,
                    bytes: Some(9 * MB + 40 * 1024),
                    scans: Some(9_802_100),
                },
                IndexStats {
                    name: "idx_orders_created".into(),
                    columns: vec!["created_at".into()],
                    unique: false,
                    bytes: Some(10 * MB + 56 * 1024),
                    scans: Some(0),
                },
            ];
        } else {
            stats.live_rows = Some(4_000);
            stats.dead_rows = Some(12);
            stats.table_bytes = Some(512 * 1024);
            stats.index_bytes = Some(232 * 1024);
            stats.total_bytes = Some(744 * 1024);
            stats.seq_scans = Some(18_410);
            stats.idx_scans = Some(9_790_402);
            stats.last_analyze = Some("2026-10-30 09:05:12+00".to_owned());
            stats.last_autoanalyze = Some("2026-10-27 14:20:31+00".to_owned());
            stats.last_vacuum = Some("2026-10-30 09:05:11+00".to_owned());
            stats.modified_since_analyze = Some(41);
            stats.stats_present = Some(true);
            stats.indexes = vec![IndexStats {
                name: "customers_pkey".into(),
                columns: vec!["id".into()],
                unique: true,
                bytes: Some(112 * 1024),
                scans: Some(9_790_402),
            }];
        }
        t.stats = Some(stats);
    }
    facts
}

/// The same query once `orders (status, created_at)` has an index: read through it, joined
/// in a nested loop, 12 ms instead of 845 — analyzed with the index known.
pub fn indexed(text: &str) -> (Arc<ProfileReport>, Facts) {
    let start = text.find("SELECT").unwrap_or(0);
    let end = text.rfind(';').map_or(text.len(), |e| e + 1);
    let statement = &text[start..end];
    let mut profile = ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .engine("PostgreSQL", Some("17.2"))
        .child(OpKind::Limit, "Limit", |n| {
            planned(n, 2_410.2, 2_410.3, 50.0);
            ran(n, 12.21, 12.24, 50.0, 1.0);
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Sort, "Sort", |n| {
            planned(n, 2_410.2, 2_413.1, 1180.0);
            ran(n, 12.2, 12.21, 50.0, 1.0);
            cond(n, CondKind::SortKey, "(sum(o.total)) DESC");
            n.memory = Some(Memory {
                sort_method: Some("top-N heapsort".into()),
                sort_space: Some((32 * 1024, Storage::Memory)),
                ..Memory::default()
            });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::Aggregate(AggStrategy::Hashed), "HashAggregate", |n| {
            planned(n, 2_380.0, 2_391.8, 1180.0);
            ran(n, 11.6, 11.9, 1180.0, 1.0);
            cond(n, CondKind::GroupKey, "c.name");
            n.memory = Some(Memory { peak_bytes: Some(409_600), ..Memory::default() });
            n.estimates.width = Some(48.0);
        })
        .child(OpKind::NestedLoop, "Nested Loop", |n| {
            planned(n, 0.85, 2_334.1, 6020.0);
            ran(n, 0.05, 9.8, 6133.0, 1.0);
            n.estimates.width = Some(26.0);
        })
        .node(OpKind::IndexScan, "Index Scan", |n| {
            on(n, "orders", Some("o"));
            if let Some(o) = n.object.as_mut() {
                o.schema = Some("public".into());
            }
            n.index = Some("idx_orders_status_created".into());
            planned(n, 0.43, 512.6, 6020.0);
            ran(n, 0.03, 3.9, 6133.0, 1.0);
            cond(
                n,
                CondKind::IndexCond,
                "(((status)::text = 'pending'::text) AND (created_at >= (now() - '30 days'::interval)))",
            );
            n.io = Some(io(6_210, 184));
            n.estimates.width = Some(22.0);
        })
        .node(OpKind::IndexScan, "Index Scan", |n| {
            on(n, "customers", Some("c"));
            if let Some(o) = n.object.as_mut() {
                o.schema = Some("public".into());
            }
            n.index = Some("customers_pkey".into());
            planned(n, 0.29, 0.31, 1.0);
            ran(n, 0.0006, 0.0007, 1.0, 6133.0);
            cond(n, CondKind::IndexCond, "(id = o.customer_id)");
            n.io = Some(io(12_266, 0));
            n.estimates.width = Some(18.0);
        })
        .with(|p| {
            p.statement.planning_ms = Some(0.94);
            p.statement.execution_ms = Some(12.38);
            p.statement.wall_ms = Some(14.1);
            p.statement.rows_returned = Some(50);
            p.collected.buffers = true;
        })
        .build();
    profile.sum_io();
    let report = database::profile::derive(profile);
    let mut facts = report_with(text, false).1;
    if let Some(orders) = facts.tables.iter_mut().find(|t| t.table.name == "orders")
        && let Some(s) = orders.structure.as_mut()
    {
        s.indexes.push(database::IndexInfo::new(
            "idx_orders_status_created",
            vec!["status".to_owned(), "created_at".to_owned()],
            false,
        ));
    }
    analyzed(report, &facts, statement, start..end)
}

/// The sample shown as `runner`'s console's (its tab revealed), `text` the console's.
pub fn show(runner: &Runner, text: &str) -> ServiceId {
    let (report, facts) = report_with(text, false);
    shown(runner, text, report, facts, 851_400)
}

/// The sample with its tables' statistics, shown as [`show`] does.
pub fn show_tables(runner: &Runner, text: &str) -> ServiceId {
    let (report, facts) = report_with(text, true);
    shown(runner, text, report, facts, 851_400)
}

/// The sample, then the same query after its index ([`indexed`]), both kept by `runner`'s
/// console: the second's tab, and the first's profile (to compare with).
pub fn show_compare(runner: &Runner, text: &str) -> (ServiceId, database::profile::ProfileId) {
    let (before, facts) = report_with(text, false);
    let first = before.profile.id;
    shown(runner, text, before, facts, 851_400);
    let (after, facts) = indexed(text);
    (shown(runner, text, after, facts, 14_100), first)
}

fn shown(
    runner: &Runner,
    text: &str,
    report: Arc<ProfileReport>,
    facts: Facts,
    micros: u64,
) -> ServiceId {
    let start = text.find("SELECT").unwrap_or(0);
    let end = text.rfind(';').map_or(text.len(), |e| e + 1);
    let statement = text[start..end].to_owned();
    runner.show_profile(ProfileShown {
        title: crate::runner::profile_title(&statement, FLAVOR),
        table: None,
        fingerprint: sql::fingerprint(&statement, FLAVOR).hash,
        statement,
        range: start..end,
        analyze: true,
        rolled_back: false,
        elapsed: std::time::Duration::from_micros(micros),
        document_hash: text.get(start..end).map(ui_components::editor_toolbar::text_hash),
        report,
        facts: Some(Arc::new(facts)),
    })
}

/// What an engine measured of `profile` (its planning, execution, first row, each step's
/// time) replaced by fixed figures — a step's share of the time by the rows it went
/// through, with its subtree's — so a real profile looks the same every run (the snapshots).
pub fn freeze_times(profile: &mut database::profile::Profile) {
    let s = &mut profile.statement;
    let execution = 3.84;
    for (slot, value) in [
        (&mut s.planning_ms, 0.214),
        (&mut s.execution_ms, execution),
        (&mut s.first_row_ms, 2.91),
        (&mut s.fetch_ms, 0.12),
    ] {
        if slot.is_some() {
            *slot = Some(value);
        }
    }
    let n = profile.nodes.len();
    // Each step's weight: its rows (and one), with its subtree's.
    let mut weight = vec![0.0f64; n];
    let order: Vec<_> = profile.walk().into_iter().map(|(id, _)| id).collect();
    for id in order.iter().rev() {
        let node = &profile.nodes[id.index()];
        let own = node.total_rows().unwrap_or(0.0) + 1.0;
        let children: f64 = node.children.iter().map(|c| weight[c.index()]).sum();
        weight[id.index()] = own + children;
    }
    let all: f64 = profile.roots.iter().map(|r| weight[r.index()]).sum::<f64>().max(1.0);
    for (i, node) in profile.nodes.iter_mut().enumerate() {
        if node.actuals.total_ms.is_some() {
            let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
            let total = execution * weight[i] / all / loops;
            node.actuals.total_ms = Some((total * 1000.0).round() / 1000.0);
            if node.actuals.startup_ms.is_some() {
                node.actuals.startup_ms = Some((total * 100.0).round() / 1000.0);
            }
        }
    }
}
