use super::*;
use crate::profile::node::{CondKind, Condition, ObjectKind, ObjectRef, OpKind, Spill, SpillKind};
use crate::profile::testing::{ProfileBuilder, planned, ran};
use crate::profile::{Extra, ExtraValue, ProfileNote, Quantity, RawFormat, Unit};

fn sample() -> Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .id(0x1234_5678_9abc_4def_8123_4567_89ab_cdef)
        .engine("PostgreSQL", Some("16.4"))
        .child(OpKind::Sort, "Sort", |n| {
            planned(n, 10.5, 10.75, 100.0);
            ran(n, 2.0, 2.5, 100.0, 1.0);
            n.conditions.push(Condition::new(CondKind::SortKey, "total DESC"));
            n.memory = Some(Memory {
                sort_method: Some("external merge".into()),
                sort_space: Some((1_048_576, crate::profile::Storage::Disk)),
                ..Memory::default()
            });
            n.spill =
                Some(Spill { kind: SpillKind::Sort, disk_bytes: Some(1_048_576), batches: None });
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            n.object = Some(ObjectRef {
                schema: Some("public".into()),
                ..ObjectRef::new(ObjectKind::Table, "orders \"big\"")
            });
            planned(n, 0.0, 8.0, 100.0);
            ran(n, 0.01, 1.5, 100.0, 1.0);
            n.conditions.push(Condition::new(CondKind::Filter, "(status <> 'x'::text)"));
            n.actuals.rows_removed_by_filter = Some(3.0);
            n.io = Some(Io { shared_hit: Some(4), shared_read: Some(2), ..Io::default() });
            n.extra.push(Extra::new("Async Capable", ExtraValue::Bool(false)));
            n.extra.push(Extra::new(
                "Unknown Thing",
                ExtraValue::Group(vec![
                    Extra::new("a", ExtraValue::Int(1)),
                    Extra::new(
                        "b",
                        ExtraValue::List(vec![
                            ExtraValue::Float(0.5),
                            ExtraValue::Text("t".into()),
                        ]),
                    ),
                    Extra::new("c", ExtraValue::Quantity(Quantity::new(2.0, Unit::Millis))),
                ]),
            ));
        })
        .with(|p| {
            p.statement.planning_ms = Some(0.12);
            p.statement.execution_ms = Some(2.6);
            p.notes.push(ProfileNote::RolledBack);
            p.keep_raw(RawFormat::Json, "[{\"Plan\": {}}]", 1000);
        })
        .build()
}

#[test]
fn the_json_starts_with_its_format_and_keeps_its_order() {
    let json = to_json(&sample());
    assert!(
        json.starts_with(
            "{\n  \"format\": 1,\n  \"id\": \"12345678-9abc-4def-8123-456789abcdef\","
        ),
        "{json}"
    );
    let order = [
        "\"engine\"",
        "\"depth\"",
        "\"statement\"",
        "\"nodes\"",
        "\"roots\"",
        "\"collected\"",
        "\"notes\"",
        "\"raw\"",
    ];
    let mut at = 0;
    for key in order {
        let found = json[at..].find(key).unwrap_or_else(|| panic!("{key} after {at} in {json}"));
        at += found;
    }
    // Escaped text, unknown groups kept with their types, units named.
    assert!(json.contains("\"name\": \"orders \\\"big\\\"\""), "{json}");
    assert!(json.contains("\"type\": \"group\""));
    assert!(json.contains("\"type\": \"quantity\",\n"));
    assert!(json.contains("\"unit\": \"ms\""));
    assert!(json.contains("\"text\": \"[{\\\"Plan\\\": {}}]\""));
    assert!(json.contains("\"id\": \"rolled_back\""));
    // The same profile, the same text (nothing depends on a map's order).
    assert_eq!(json, to_json(&sample()));
    // Only what's known is written: no `null` for an unknown figure.
    assert_eq!(json.matches("null").count(), 1, "only the root's parent");
}

#[test]
fn the_text_reads_as_explain_does() {
    let text = to_text(&sample());
    let expected = "\
-- PostgreSQL 16.4 · actual · format 1
Sort  (cost=10.50..10.75 rows=100) (actual time=2.000..2.500 rows=100 loops=1)
  Sort Key: total DESC
  Sort Method: external merge  disk: 1048576 bytes
  Spill: sort disk=1048576 bytes
  ->  Seq Scan on public.orders \"big\"  (cost=0.00..8.00 rows=100) (actual time=0.010..1.500 rows=100 loops=1)
        Filter: (status <> 'x'::text)
        Rows Removed by Filter: 3
        Buffers: shared hit=4 read=2
        Async Capable: false
        Unknown Thing: {a: 1, b: [0.5, t], c: 2 ms}
Planning Time: 0.120 ms
Execution Time: 2.600 ms
Note: rolled_back
";
    assert_eq!(text, expected);
}

#[test]
fn numbers_are_written_plainly() {
    assert_eq!(number(5.0), "5");
    assert_eq!(number(0.04), "0.04");
    assert_eq!(number(12.3456), "12.346");
    assert_eq!(number(f64::NAN), "0");
    assert_eq!(number(-0.0001), "0");
    assert_eq!(Json::Num(f64::INFINITY).text(), "null");
    assert_eq!(
        depth_from_id(depth_id(ProfileDepth::Actual { timing: false })),
        Some(ProfileDepth::Actual { timing: false })
    );
}
