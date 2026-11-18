use super::*;
use crate::{FkAction, ForeignKey, IndexInfo};

fn column(name: &str, ty: &str, key: bool) -> ColumnInfo {
    let mut c = ColumnInfo::new(name, ty);
    c.in_key = key;
    c
}

fn key(columns: &[&str], table: TableRef, references: &[&str]) -> ForeignKey {
    ForeignKey {
        name: None,
        columns: columns.iter().map(|c| (*c).to_owned()).collect(),
        table,
        references: references.iter().map(|c| (*c).to_owned()).collect(),
        on_update: FkAction::NoAction,
        on_delete: FkAction::NoAction,
    }
}

fn table(name: &str, columns: Vec<ColumnInfo>, keys: Vec<ForeignKey>) -> (TableInfo, Structure) {
    let table = TableRef::new(name);
    let info = TableInfo { table: table.clone(), kind: TableKind::Table, rows: Some(3) };
    (info, Structure { table, columns, foreign_keys: keys, ..Structure::default() })
}

fn shop() -> SchemaGraph {
    SchemaGraph::new(vec![
        table(
            "orders",
            vec![column("id", "INTEGER", true), column("user_id", "INTEGER", false)],
            vec![key(&["user_id"], TableRef::new("users"), &["id"])],
        ),
        table("users", vec![column("id", "INTEGER", true), column("name", "TEXT", false)], vec![]),
        table(
            "profiles",
            vec![column("user_id", "INTEGER", true), column("bio", "TEXT", false)],
            vec![key(&["user_id"], TableRef::new("USERS"), &["id"])],
        ),
        table(
            "categories",
            vec![column("id", "INTEGER", true), column("parent_id", "INTEGER", false)],
            vec![key(&["parent_id"], TableRef::new("categories"), &["id"])],
        ),
        table(
            "lines",
            vec![
                column("order_id", "INTEGER", true),
                column("n", "INTEGER", true),
                column("sku", "TEXT", false),
            ],
            vec![
                key(&["order_id"], TableRef::new("orders"), &["id"]),
                key(&["sku"], TableRef::in_schema("stock", "products"), &["sku"]),
            ],
        ),
    ])
}

#[test]
fn foreign_keys_become_links_between_the_tables() {
    let g = shop();
    let names: Vec<&str> = g.tables.iter().map(|t| t.table().name.as_str()).collect();
    // Sorted (the external table last: a schema of its own).
    assert_eq!(names, ["categories", "lines", "orders", "profiles", "users", "products"]);
    let link = |from: &str| {
        let i = g.tables.iter().position(|t| t.table().name == from).unwrap();
        g.links.iter().filter(move |l| l.from == i).collect::<Vec<_>>()
    };
    let orders = link("orders");
    assert_eq!(g.label(orders[0]), "orders.user_id → users.id");
    assert_eq!(orders[0].cardinality, Cardinality::OneToMany);
    // The key is the table's whole primary key: one to one (the name in any case).
    let profiles = link("profiles");
    assert_eq!(g.tables[profiles[0].to].table().name, "users");
    assert_eq!(profiles[0].cardinality, Cardinality::OneToOne);
    // Part of a composite key: many.
    assert_eq!(link("lines")[0].cardinality, Cardinality::OneToMany);
    // A tree.
    assert!(link("categories")[0].is_self());
}

#[test]
fn a_target_not_read_is_an_external_table_with_the_columns_named() {
    let g = shop();
    let products = g.tables.last().unwrap();
    assert!(products.external);
    assert_eq!(products.table(), &TableRef::in_schema("stock", "products"));
    assert_eq!(products.structure.columns[0].declared_type, "TEXT");
    assert_eq!(
        g.label(g.links.iter().find(|l| g.tables[l.to].external).unwrap()),
        "lines.sku → products.sku"
    );
}

#[test]
fn cross_schema_keys_find_their_table_and_unique_indexes_make_one_to_one() {
    let mut users = table("users", vec![column("id", "int", true)], vec![]);
    users.0.table = TableRef::in_schema("auth", "users");
    let mut account = table(
        "accounts",
        vec![column("id", "int", true), column("user_id", "int", false)],
        vec![key(&["user_id"], TableRef::in_schema("auth", "users"), &["id"])],
    );
    account.0.table = TableRef::in_schema("billing", "accounts");
    account.1.indexes.push(IndexInfo::new("one", vec!["user_id".into()], true));
    let g = SchemaGraph::new(vec![users, account]);
    assert_eq!(g.links.len(), 1);
    assert!(!g.tables.iter().any(|t| t.external));
    assert_eq!(g.links[0].cardinality, Cardinality::OneToOne);
}

#[test]
fn composite_keys_are_one_link() {
    let parent = table("pairs", vec![column("a", "int", true), column("b", "int", true)], vec![]);
    let child = table(
        "uses",
        vec![column("id", "int", true), column("a", "int", false), column("b", "int", false)],
        vec![key(&["a", "b"], TableRef::new("pairs"), &["a", "b"])],
    );
    let g = SchemaGraph::new(vec![parent, child]);
    assert_eq!(g.links.len(), 1);
    assert_eq!(g.label(&g.links[0]), "uses.(a, b) → pairs.(a, b)");
}

#[test]
fn neighborhood_reaches_as_far_as_asked() {
    let g = shop();
    let find = |name: &str| g.find(&TableRef::new(name)).unwrap();
    let names = |set: Vec<usize>| -> Vec<String> {
        set.into_iter().map(|i| g.tables[i].table().name.clone()).collect()
    };
    assert_eq!(names(g.neighborhood(find("users"), 0, |_| true)), ["users"]);
    assert_eq!(names(g.neighborhood(find("users"), 1, |_| true)), ["orders", "profiles", "users"]);
    assert_eq!(
        names(g.neighborhood(find("users"), 2, |_| true)),
        ["lines", "orders", "profiles", "users"]
    );
    assert_eq!(
        names(g.neighborhood(find("users"), 1, |l| l.cardinality == Cardinality::OneToOne)),
        ["profiles", "users"]
    );
    assert!(g.neighborhood(99, 1, |_| true).is_empty());
}

#[test]
fn matching_filters_by_name_any_case() {
    let g = shop();
    assert_eq!(g.matching("").len(), g.tables.len());
    let found: Vec<&str> =
        g.matching("ORD").iter().map(|&i| g.tables[i].table().name.as_str()).collect();
    assert_eq!(found, ["orders"]);
    assert_eq!(g.matching("stock.").len(), 1);
}

/// Thousands of tables, each pointing to the one before and to one only named (twice): each
/// key found among the tables of its name, the named one kept once, quickly.
#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn a_big_schema_s_keys_are_resolved_by_name() {
    let n = 4000;
    let tables: Vec<_> = (0..n)
        .map(|i| {
            let mut keys = vec![key(&["ext_id"], TableRef::new("Outside"), &["id"])];
            if i > 0 {
                keys.push(key(&["prev_id"], TableRef::new(format!("T{:05}", i - 1)), &["id"]));
            }
            table(&format!("t{i:05}"), vec![column("id", "int", true)], keys)
        })
        .collect();
    let started = std::time::Instant::now();
    let g = SchemaGraph::new(tables);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(g.tables.len(), n + 1, "the named one added once");
    assert_eq!(g.links.len(), n * 2 - 1);
    let prev = g
        .links
        .iter()
        .find(|l| g.tables[l.from].table().name == "t00007" && l.columns[0] == "prev_id");
    assert_eq!(prev.map(|l| g.tables[l.to].table().name.as_str()), Some("t00006"), "any case");
}
