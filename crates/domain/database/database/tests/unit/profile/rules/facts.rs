use super::*;
use crate::profile::testing::table;
use crate::relations::VirtualKey;
use crate::{ColumnInfo, ForeignKey, Structure};

#[test]
fn indexes_by_their_leading_columns() {
    let t = table(
        "orders",
        &["id:integer", "a", "b"],
        &[("ab", &["a", "b"]), ("expr", &["lower((a)::text)"])],
    );
    assert!(t.knows_indexes());
    let names = |c: &[&str]| {
        t.index_starting_with(&c.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>())
            .map(|i| i.name)
    };
    assert_eq!(names(&["a"]).as_deref(), Some("ab"));
    assert_eq!(names(&["A", "B"]).as_deref(), Some("ab"), "without case");
    assert_eq!(names(&["b"]), None, "not its first column");
    assert_eq!(names(&[]), None);
    assert_eq!(t.expression_index_on("a").map(|i| i.name).as_deref(), Some("expr"));
    assert!(t.expression_index_on("b").is_none());
    assert_eq!(t.has_column("B"), Some(true));
    assert_eq!(TableFacts::new(TableRef::new("x")).has_column("a"), None, "unknown");
}

#[test]
fn sqlites_integer_primary_key_is_an_index() {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let t = TableFacts {
        structure: Some(Structure {
            table: TableRef::new("t"),
            columns: vec![id],
            rowid: true,
            ..Structure::default()
        }),
        ..TableFacts::new(TableRef::new("t"))
    };
    assert!(t.index_starting_with(&["id".into()]).is_some());
}

#[test]
fn foreign_keys_the_engines_and_the_accepted() {
    let mut orders = table("orders", &["customer_id:integer", "seller_id:integer"], &[]);
    orders.structure.as_mut().unwrap().foreign_keys.push(ForeignKey {
        name: None,
        columns: vec!["customer_id".into()],
        table: TableRef::new("customers"),
        references: vec!["id".into()],
        on_update: Default::default(),
        on_delete: Default::default(),
    });
    let mut facts = Facts { tables: vec![orders], ..Facts::default() };
    facts.notes.accept(VirtualKey {
        table: TableRef::new("orders"),
        columns: vec!["seller_id".into()],
        target: TableRef::new("users"),
        references: vec!["id".into()],
    });
    let keys = facts.foreign_keys(&TableRef::new("ORDERS"));
    assert_eq!(keys.len(), 2);
    assert!(!keys[0].accepted && keys[1].accepted);
    assert_eq!(keys[1].target, TableRef::new("users"));
    assert!(facts.foreign_keys(&TableRef::new("users")).is_empty());
    // A schema on one side only still names the same table.
    assert!(facts.table(&TableRef::in_schema("public", "orders")).is_some());
    assert!(same_table(&TableRef::in_schema("a", "t"), &TableRef::new("T")));
    assert!(!same_table(&TableRef::in_schema("a", "t"), &TableRef::in_schema("b", "t")));
}

#[test]
fn a_profiles_tables_with_aliases_taken_back() {
    use crate::profile::node::OpKind;
    use crate::profile::testing::{ProfileBuilder, on};
    let p = ProfileBuilder::new("sqlite", crate::profile::ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "SCAN c", |n| on(n, "c", None))
        .node(OpKind::IndexScan, "SEARCH o", |n| on(n, "orders", Some("o")))
        .build();
    let map = SqlMap::new(
        "SELECT * FROM customers c JOIN orders o ON o.cid = c.id JOIN items i ON i.oid = o.id",
        crate::sql::Flavor::Sqlite,
    );
    let tables = tables_of(&p, &map);
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["customers", "orders", "items"], "each once, the statement's too");
}

#[test]
fn recent_runs_are_counted_in_their_window_and_bounded() {
    let mut recent = RecentRuns::default();
    for i in 0..30 {
        recent.record(7, Duration::from_secs(i * 3));
    }
    recent.record(8, Duration::from_secs(90));
    assert_eq!(recent.count_within(7, Duration::from_secs(60)), 20, "from 30 s (90 − 60) to 87 s");
    assert_eq!(recent.count_within(9, Duration::from_secs(60)), 0);
    for i in 0..(RecentRuns::TIMES as u64 + 10) {
        recent.record(1, Duration::from_secs(100) + Duration::from_millis(i));
    }
    assert_eq!(recent.count_within(1, Duration::from_secs(600)), RecentRuns::TIMES);
    for h in 0..(RecentRuns::KEEP as u64 + 5) {
        recent.record(1_000 + h, Duration::from_secs(200));
    }
    assert_eq!(recent.count_within(7, Duration::from_secs(1_000)), 0, "the oldest went");
    assert!(!recent.is_empty());
}
