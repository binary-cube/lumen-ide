use std::collections::BTreeMap;

use database::{
    CellChange, Change, ChangeSet, ColumnDef, ColumnInfo, CompareOp, DefaultValue, FkAction,
    ForeignKey, Generated, IndexInfo, KeyKind, Order, Page, RowKey, SchemaChange, Structure,
    TableDef, TableRef, Value,
};

use super::*;

/// Through JSON and back: what the other side reads is what was sent.
fn across<T, W>(value: T) -> T
where
    T: Clone + Into<W>,
    W: serde::Serialize + serde::de::DeserializeOwned + Into<T>,
{
    let wire: W = value.into();
    let json = serde_json::to_string(&wire).unwrap();
    let back: W = serde_json::from_str(&json).unwrap();
    back.into()
}

fn users() -> TableRef {
    TableRef::in_schema("main", "users")
}

#[test]
fn a_structure_crosses_whole() {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    id.key_position = Some(1);
    id.nullable = false;
    id.auto_increment = true;
    let mut total = ColumnInfo::new("total", "NUMERIC(10,2)");
    total.default = Some("0".into());
    total.collation = Some("C".into());
    total.generated = Some(Generated { expression: "a + b".into(), stored: true });
    let structure = Structure {
        table: users(),
        columns: vec![id, total, ColumnInfo::new("data", "jsonb")],
        indexes: vec![IndexInfo {
            name: "by_total".into(),
            columns: vec!["total".into()],
            unique: true,
            implicit: false,
            descending: vec![true],
            predicate: Some("total > 0".into()),
        }],
        foreign_keys: vec![ForeignKey {
            name: Some("fk".into()),
            columns: vec!["team".into()],
            table: TableRef::new("teams"),
            references: vec!["id".into()],
            on_update: FkAction::Cascade,
            on_delete: FkAction::SetNull,
        }],
        rowid: true,
        lines: false,
    };
    assert_eq!(across::<_, dto::Structure>(structure.clone()), structure);
}

#[test]
fn a_page_with_its_filter_and_a_change_set_cross_whole() {
    let page = Page {
        offset: 500,
        limit: 100,
        order: vec![Order { column: "name".into(), descending: true }],
        filter: Some(database::Filter::And(vec![
            database::Filter::Compare {
                column: "score".into(),
                op: CompareOp::GreaterEq,
                value: Value::Float(2.5),
            },
            database::Filter::Not(Box::new(database::Filter::IsNull("name".into()))),
            database::Filter::Or(vec![
                database::Filter::Contains { column: "name".into(), text: "an".into() },
                database::Filter::NotNull("email".into()),
            ]),
        ])),
    };
    assert_eq!(across::<_, dto::Page>(page.clone()), page);
    let key = RowKey::Primary(vec![("id".into(), Value::Int(i64::MAX))]);
    let set = ChangeSet {
        table: users(),
        changes: vec![
            Change::Insert {
                values: vec![
                    ("name".into(), Value::Text("ana".into())),
                    ("photo".into(), Value::Bytes(vec![0, 1, 254, 255])),
                    ("meta".into(), Value::Json("{\"a\":1}".into())),
                    ("born".into(), Value::Other("2001-02-03".into())),
                    ("ok".into(), Value::Bool(true)),
                    ("none".into(), Value::Null),
                ],
            },
            Change::Update {
                key,
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("a".into()),
                    new: Value::Text("b".into()),
                }],
            },
            Change::Delete { key: RowKey::Line { line: 7, fingerprint: u64::MAX } },
            Change::Delete { key: RowKey::RowId(-3) },
        ],
    };
    assert_eq!(across::<_, dto::ChangeSet>(set.clone()), set);
}

#[test]
fn structure_changes_and_a_planned_script_cross_whole() {
    let column = ColumnDef::new("email", "TEXT")
        .not_null()
        .default(DefaultValue::Value(Value::Text("x".into())));
    let changes = vec![
        SchemaChange::CreateTable(TableDef {
            table: TableRef::new("t"),
            columns: vec![
                ColumnDef::new("id", "INTEGER").default(DefaultValue::CurrentTimestamp),
                ColumnDef::new("e", "TEXT").default(DefaultValue::Expression("lower('A')".into())),
            ],
            primary_key: vec!["id".into()],
            foreign_keys: Vec::new(),
            uniques: vec![vec!["e".into()]],
            checks: vec!["id > 0".into()],
            without_rowid: true,
        }),
        SchemaChange::RenameTable { table: users(), to: "people".into() },
        SchemaChange::DropTable { table: users() },
        SchemaChange::AddColumn { table: users(), column: column.clone() },
        SchemaChange::RenameColumn { table: users(), from: "a".into(), to: "b".into() },
        SchemaChange::AlterColumn { table: users(), column: "a".into(), to: column },
        SchemaChange::DropColumn { table: users(), column: "a".into() },
        SchemaChange::CreateIndex {
            table: users(),
            index: IndexInfo::new("i", vec!["a".into()], false),
        },
        SchemaChange::DropIndex { table: users(), name: "i".into() },
        SchemaChange::MoveColumn { table: users(), column: "a".into(), to: 2 },
    ];
    for change in changes {
        assert_eq!(across::<_, dto::SchemaChange>(change.clone()), change);
    }
    let script = database::Script {
        before: vec!["PRAGMA foreign_keys = OFF".into()],
        begin: "BEGIN".into(),
        steps: vec![
            database::Step { sql: "a".into(), expect: database::Expect::OneRow },
            database::Step {
                sql: "b".into(),
                expect: database::Expect::Inserted(KeyKind::Primary(vec!["id".into()])),
            },
            database::Step {
                sql: "c".into(),
                expect: database::Expect::NoNewViolations { tables: vec!["users".into()] },
            },
        ],
        commit: "COMMIT".into(),
        rollback: "ROLLBACK".into(),
        after: vec!["PRAGMA foreign_keys = ON".into()],
        stale: vec![database::sqlite::Stale {
            table: users(),
            kind: database::sqlite::ObjectKind::View,
            name: "named".into(),
        }],
    };
    assert_eq!(across::<_, dto::Script>(script.clone()), script);
}

#[test]
fn a_connection_spec_and_a_target_cross_and_a_type_class_is_told_when_not_given() {
    let spec = database::ConnectionSpec::server(5432, &["postgresql"]).with(
        database::FieldSpec::new(
            "mode",
            "Mode",
            database::FieldKind::Choice(vec![("a".into(), "A".into())]),
            database::Binding::Option("mode".into()),
            database::FieldTab::Options,
        )
        .required(),
    );
    assert_eq!(across::<_, dto::Connection>(spec.clone()), spec);
    let mut options = BTreeMap::new();
    options.insert("sslmode".to_owned(), "require".to_owned());
    let target = database::Target {
        host: "db".into(),
        port: Some(5432),
        user: "ana".into(),
        database: "shop".into(),
        options,
        tls: database::Tls::Verify,
    };
    let wire = dto::Target::from(&target);
    let back: dto::Target = serde_json::from_str(&serde_json::to_string(&wire).unwrap()).unwrap();
    assert_eq!(database::Target::from(back), target);
    let column: dto::ColumnInfo =
        serde_json::from_str(r#"{"name": "n", "declared_type": "bigint"}"#).unwrap();
    let column = database::ColumnInfo::from(column);
    assert_eq!(column.class, database::TypeClass::Integer);
    assert!(column.nullable, "nullable unless said");
    let node = database::Node::from(dto::Node {
        kind: "galaxy".into(),
        name: "x".into(),
        detail: String::new(),
        path: Vec::new(),
        expandable: false,
    });
    assert_eq!(node.kind, "other", "a kind the editor doesn't know");
}

mod profiles {
    use database::profile::testing::{ProfileBuilder, planned, ran};
    use database::profile::{
        ChildRole, CondKind, Condition, EngineCounters, Extra, ExtraValue, Io, Jit, Memory,
        ObjectKind, ObjectRef, OpKind, Parallel, Profile, ProfileDepth, ProfileNote, Quantity,
        RawFormat, Spill, SpillKind, Storage, TableStats, Unit, Wal, WorkerStats, export,
    };

    use super::*;

    /// A profile with something in every part of the model.
    fn full() -> Profile {
        ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
            .engine("PostgreSQL", Some("17.2"))
            .child(OpKind::Gather, "Gather", |n| {
                ran(n, 1.0, 9.5, 300.0, 1.0);
                planned(n, 1000.0, 2000.5, 290.0);
                n.estimates.width = Some(12.0);
                n.parallel = Some(Parallel {
                    planned: Some(2),
                    launched: Some(2),
                    aware: false,
                    per_worker: vec![WorkerStats {
                        number: 0,
                        rows: Some(100.0),
                        loops: Some(1.0),
                        startup_ms: Some(0.5),
                        total_ms: Some(8.0),
                        io: Some(Io { shared_hit: Some(3), ..Io::default() }),
                        extra: vec![Extra::new("Worker Mood", ExtraValue::Text("ok".into()))],
                    }],
                });
            })
            .node(OpKind::Sort, "Sort", |n| {
                n.role = Some(ChildRole::Outer);
                n.subplan_name = Some("CTE x".into());
                n.join = Some(database::profile::JoinKind::RightAnti);
                n.object = Some(ObjectRef {
                    kind: ObjectKind::Table,
                    schema: Some("s".into()),
                    name: "t".into(),
                    alias: Some("a".into()),
                });
                n.index = Some("t_pkey".into());
                n.scan_direction = Some(database::profile::ScanDirection::Backward);
                n.conditions = vec![
                    Condition::new(CondKind::SortKey, "a DESC"),
                    Condition::new(CondKind::Other("Order By".into()), "x"),
                    Condition::new(CondKind::Other(String::new()), "raw"),
                ];
                n.output = vec!["a".into(), "b".into()];
                n.actuals.never_executed = true;
                n.actuals.rows_removed_by_filter = Some(4.0);
                n.actuals.index_searches = Some(2.0);
                n.io = Some(Io {
                    temp_read: Some(9),
                    temp_write_ms: Some(1.5),
                    block_size: Some(8192),
                    ..Io::default()
                });
                n.memory = Some(Memory {
                    peak_bytes: Some(1024),
                    sort_method: Some("external merge".into()),
                    sort_space: Some((4096, Storage::Disk)),
                    hash: Some(database::profile::HashStats {
                        batches: Some(4),
                        ..Default::default()
                    }),
                    cache: Some(database::profile::CacheStats {
                        hits: Some(1),
                        ..Default::default()
                    }),
                    storage: Some(Storage::Memory),
                });
                n.spill =
                    Some(Spill { kind: SpillKind::Sort, disk_bytes: Some(4096), batches: None });
                n.wal = Some(Wal { records: Some(1), bytes: Some(70), ..Wal::default() });
                n.flags.disabled = true;
                n.flags.partial = true;
                n.extra = vec![
                    Extra::new("b", ExtraValue::Bool(true)).labelled("B").in_group("G"),
                    Extra::new("i", ExtraValue::Int(-3)),
                    Extra::new("f", ExtraValue::Float(0.5)),
                    Extra::new("t", ExtraValue::Text("x".into())),
                    Extra::new("q", ExtraValue::Quantity(Quantity::new(2.5, Unit::Bytes))),
                    Extra::new(
                        "l",
                        ExtraValue::List(vec![ExtraValue::Int(1), ExtraValue::List(vec![])]),
                    ),
                    Extra::new(
                        "g",
                        ExtraValue::Group(vec![Extra::new("in", ExtraValue::Float(1.25))]),
                    ),
                ];
            })
            .with(|p| {
                let s = &mut p.statement;
                s.planning_ms = Some(0.1);
                s.execution_ms = Some(9.6);
                s.wall_ms = Some(12.0);
                s.first_row_ms = Some(3.0);
                s.fetch_ms = Some(0.2);
                s.rows_returned = Some(300);
                s.rows_affected = Some(0);
                s.planning_io = Some(Io { shared_hit: Some(5), ..Io::default() });
                s.planning_memory = Some(database::profile::PlanningMemory {
                    used_bytes: Some(10),
                    allocated_bytes: Some(20),
                });
                s.serialize = Some(database::profile::Serialize {
                    ms: Some(0.3),
                    bytes: Some(100),
                    format: Some("text".into()),
                });
                s.jit = Some(Jit { functions: Some(4), total_ms: Some(2.0), ..Jit::default() });
                s.triggers.push(database::profile::TriggerTime {
                    name: "tr".into(),
                    relation: Some("t".into()),
                    calls: Some(2),
                    ms: Some(0.4),
                });
                s.settings.push(database::profile::Setting {
                    name: "work_mem".into(),
                    value: "64MB".into(),
                });
                s.query_id = Some(-42);
                s.counters =
                    Some(EngineCounters { vm_steps: Some(9), ..EngineCounters::default() });
                s.history = Some(database::profile::StatementStats {
                    calls: Some(7),
                    mean_ms: Some(1.5),
                    ..Default::default()
                });
                s.waits.push(database::profile::Wait {
                    event: "io".into(),
                    ms: Some(0.1),
                    count: Some(1),
                });
                s.extra.push(Extra::new("Statement Mood", ExtraValue::Text("calm".into())));
                p.notes = vec![ProfileNote::RolledBack, ProfileNote::Truncated { nodes: 3 }];
                p.keep_raw(RawFormat::Json, "[{}]", 100);
                p.sum_io();
            })
            .build()
    }

    #[test]
    fn a_profile_goes_across_and_back_unchanged() {
        let profile = full();
        let wire = dto::Profile::from(&profile);
        let json = serde_json::to_string(&wire).unwrap();
        let back = profile_from(serde_json::from_str(&json).unwrap());
        assert_eq!(back, profile);
        assert_eq!(wire.format, database::profile::PROFILE_FORMAT, "its own format number");
    }

    /// The editor's exported JSON reads as the protocol's profile: the same names.
    #[test]
    fn the_exported_json_is_the_protocols_profile() {
        let profile = full();
        let exported = export::to_json(&profile);
        let read: dto::Profile = serde_json::from_str(&exported).unwrap();
        assert_eq!(profile_from(read), profile);
    }

    #[test]
    fn a_newer_format_is_read_as_far_as_its_understood() {
        let json = r#"{
            "format": 9,
            "engine": {"driver": "future", "name": "Future DB"},
            "depth": "actual",
            "nodes": [
                {"parent": null, "children": [1, 7], "op": "quantum_scan", "name": "Quantum Scan",
                 "role": "entangled", "object": {"kind": "wormhole", "name": "w"},
                 "conditions": [{"kind": "vibe", "text": "good"}],
                 "spill": {"kind": "elsewhere"},
                 "extra": [{"key": "k", "value": {"type": "tensor", "value": [1, 2]}}],
                 "brand_new_field": {"x": 1}},
                {"parent": 0, "children": [], "op": "seq_scan", "name": "Seq Scan"}
            ],
            "roots": [0, 5],
            "notes": [{"id": "rolled_back"}, {"id": "from_the_future"}],
            "raw": {"format": "hologram", "text": "?"}
        }"#;
        let profile = profile_from(serde_json::from_str(json).unwrap());
        assert!(profile.has_note(ProfileNote::NewerFormat { format: 9 }));
        assert!(profile.has_note(ProfileNote::RolledBack));
        assert_eq!(profile.roots.len(), 1, "a root past the list is dropped");
        let first = &profile.nodes[0];
        assert_eq!((first.op, first.name.as_str()), (OpKind::Other, "Quantum Scan"));
        assert_eq!(first.children.len(), 1);
        assert_eq!(first.role, None);
        assert_eq!(first.object.as_ref().map(|o| o.kind), Some(ObjectKind::Other));
        assert_eq!(first.conditions[0].kind, CondKind::Other("vibe".into()));
        assert_eq!(first.spill, None);
        assert_eq!(first.extra[0].value, ExtraValue::Text("[1,2]".into()), "kept as text");
        assert_eq!(profile.raw, None);
        assert_eq!(profile.walk().len(), 2);
    }

    #[test]
    fn options_caps_and_table_stats_go_across() {
        let options = database::profile::ProfileOptions {
            depth: ProfileDepth::Actual { timing: false },
            verbose: true,
            timeout: Some(std::time::Duration::from_secs(300)),
            ..database::profile::ProfileOptions::ESTIMATED
        };
        let back: database::profile::ProfileOptions = serde_json::from_str::<dto::ProfileOptions>(
            &serde_json::to_string(&dto::ProfileOptions::from(&options)).unwrap(),
        )
        .unwrap()
        .into();
        assert_eq!(back, options);
        let mut caps = database::ProfileCaps::NONE;
        caps.actual = true;
        caps.node_actuals_by_scan = true;
        assert_eq!(across::<database::ProfileCaps, dto::ProfileCaps>(caps), caps);
        let mut stats = TableStats::new(TableRef::in_schema("s", "t"));
        stats.live_rows = Some(5);
        stats.last_analyze = Some("2026-01-01T00:00:00Z".into());
        stats.indexes.push(database::profile::IndexStats {
            name: "i".into(),
            columns: vec!["a".into()],
            unique: true,
            bytes: Some(8192),
            scans: Some(3),
        });
        stats.extra.push(Extra::new("last_seq_scan", ExtraValue::Text("2026-01-02".into())));
        let wire = dto::TableStats::from(&stats);
        let back: database::profile::TableStats =
            serde_json::from_str::<dto::TableStats>(&serde_json::to_string(&wire).unwrap())
                .unwrap()
                .into();
        assert_eq!(back, stats);
        // A console said without its profiles: what its plans give.
        let said: dto::ConsoleCaps =
            serde_json::from_str(r#"{"explain": {"plan": true}}"#).unwrap();
        let console: database::ConsoleCaps = said.into();
        assert_eq!(console.profile, database::ProfileCaps::from_explain(console.explain));
    }
}
