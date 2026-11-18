use super::*;
use crate::relations::VirtualKey;
use crate::{FkAction, TableKind};

fn column(name: &str, type_name: &str, key: Option<u16>) -> ColumnBrief {
    ColumnBrief {
        name: name.into(),
        type_name: type_name.into(),
        nullable: key.is_none(),
        default: None,
        comment: None,
        key_position: key,
        values: None,
    }
}

fn fk(columns: &[&str], table: TableRef, references: &[&str]) -> ForeignKey {
    ForeignKey {
        name: None,
        columns: columns.iter().map(|c| (*c).to_owned()).collect(),
        table,
        references: references.iter().map(|c| (*c).to_owned()).collect(),
        on_update: FkAction::NoAction,
        on_delete: FkAction::NoAction,
    }
}

fn info(table: TableRef, kind: TableKind) -> TableInfo {
    TableInfo { table, kind, rows: Some(10) }
}

/// A shop: users, orders (→ users), order_items (→ orders; `product_id` only by name),
/// products, events (`user_id` only by name), categories (a tree by `parent_id`), a view.
pub(crate) fn shop(complete: bool) -> IndexParts {
    let t = TableRef::new;
    let tables = vec![
        info(t("users"), TableKind::Table),
        info(t("orders"), TableKind::Table),
        info(t("order_items"), TableKind::Table),
        info(t("products"), TableKind::Table),
        info(t("events"), TableKind::Table),
        info(t("categories"), TableKind::Table),
        info(t("big_orders"), TableKind::View),
    ];
    let columns = vec![
        (
            t("users"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("name", "TEXT", None),
                column("email", "TEXT", None),
            ],
        ),
        (
            t("orders"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("user_id", "INTEGER", None),
                column("total", "REAL", None),
                column("created_at", "TEXT", None),
            ],
        ),
        (
            t("order_items"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("order_id", "INTEGER", None),
                column("product_id", "INTEGER", None),
                column("qty", "INTEGER", None),
            ],
        ),
        (
            t("products"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("name", "TEXT", None),
                column("price", "REAL", None),
            ],
        ),
        (
            t("events"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("kind", "TEXT", None),
                column("at", "TEXT", None),
                column("user_id", "INTEGER", None),
            ],
        ),
        (
            t("categories"),
            vec![
                column("id", "INTEGER", Some(1)),
                column("parent_id", "INTEGER", None),
                column("name", "TEXT", None),
            ],
        ),
        (t("big_orders"), vec![column("id", "INTEGER", None), column("total", "REAL", None)]),
    ];
    let keys = vec![
        (t("orders"), vec![fk(&["user_id"], t("users"), &["id"])]),
        // SQLite's `REFERENCES orders` without the column: the target's key.
        (t("order_items"), vec![fk(&["order_id"], t("orders"), &[])]),
    ];
    IndexParts {
        source: DataSourceId::from_u128(7),
        database: None,
        default_schema: Some("main".into()),
        schemas: vec!["main".into()],
        tables,
        columns: if complete { columns } else { Vec::new() },
        keys: if complete { keys } else { Vec::new() },
        routines: vec![
            Routine {
                name: "count".into(),
                kind: RoutineKind::Aggregate,
                arity: Some(1),
                builtin: true,
                ..Routine::default()
            },
            Routine { name: "strftime".into(), arity: None, builtin: true, ..Routine::default() },
        ],
        readable: Vec::new(),
        notes: RelationNotes::default(),
        complete,
        generation: 1,
    }
}

/// The shop's columns and keys, as a second tier would read them.
pub(crate) fn shop_loaded(names: &[&str]) -> Vec<(TableRef, Vec<ColumnBrief>, Vec<ForeignKey>)> {
    let full = shop(true);
    full.columns
        .into_iter()
        .filter(|(t, _)| names.contains(&t.name.as_str()))
        .map(|(t, c)| {
            let keys =
                full.keys.iter().find(|(k, _)| *k == t).map(|(_, k)| k.clone()).unwrap_or_default();
            (t, c, keys)
        })
        .collect()
}

#[test]
fn tables_are_found_by_name_and_schema_in_any_case() {
    let index = SchemaIndex::build(shop(true));
    let users = index.find(&TableRef::new("users")).unwrap();
    assert_eq!(index.table(users).table, TableRef::new("users"));
    assert_eq!(index.find(&TableRef::new("USERS")), Some(users));
    assert_eq!(index.find(&TableRef::in_schema("main", "users")), Some(users));
    assert_eq!(index.find(&TableRef::in_schema("MAIN", "Users")), Some(users));
    assert_eq!(index.find(&TableRef::in_schema("temp", "users")), None);
    assert_eq!(index.find(&TableRef::new("nope")), None);
    assert!(index.in_schema(users, None));
    assert!(index.in_schema(users, Some("main")));
    assert_eq!(index.schema_of(users), Some("main"));
}

#[test]
fn the_default_schema_wins_among_same_named_tables() {
    let mut parts = IndexParts {
        default_schema: Some("public".into()),
        schemas: vec!["billing".into(), "public".into()],
        tables: vec![
            info(TableRef::in_schema("billing", "users"), TableKind::Table),
            info(TableRef::in_schema("public", "users"), TableKind::Table),
            info(TableRef::in_schema("billing", "invoices"), TableKind::Table),
        ],
        complete: true,
        ..IndexParts::default()
    };
    parts.columns =
        vec![(TableRef::in_schema("public", "users"), vec![column("id", "integer", Some(1))])];
    let index = SchemaIndex::build(parts);
    let found = index.find(&TableRef::new("users")).unwrap();
    assert_eq!(index.table(found).table, TableRef::in_schema("public", "users"));
    let invoices = index.find(&TableRef::new("invoices")).unwrap();
    assert_eq!(
        index.table(invoices).table.schema.as_deref(),
        Some("billing"),
        "the only one so named"
    );
    assert!(!index.in_schema(invoices, None));
}

#[test]
fn prefixes_are_found_first_then_fuzzy_matches() {
    let index = SchemaIndex::build(shop(true));
    let names = |q: &str| -> Vec<String> {
        index.candidates(q, 10).into_iter().map(|i| index.table(i).table.name.clone()).collect()
    };
    assert_eq!(
        names("ord"),
        ["order_items", "orders", "big_orders"],
        "starting with it, then a word"
    );
    assert_eq!(names("u"), ["users"]);
    assert_eq!(names("ordi")[0], "order_items", "fuzzy");
    assert!(names("items").contains(&"order_items".to_owned()), "a word of the name");
    assert_eq!(names("").len(), 7);
    assert_eq!(index.candidates("", 3).len(), 3, "at most the limit");
    assert!(names("zzz").is_empty());
}

#[test]
fn relations_are_the_keys_the_accepted_ones_and_those_names_say() {
    let mut parts = shop(true);
    // `product_id` accepted as a key; `user_id` of events set aside.
    parts.notes.accept(VirtualKey {
        table: TableRef::in_schema("main", "order_items"),
        columns: vec!["product_id".into()],
        target: TableRef::in_schema("main", "products"),
        references: vec!["id".into()],
    });
    parts.notes.ignore(VirtualKey {
        table: TableRef::in_schema("main", "events"),
        columns: vec!["user_id".into()],
        target: TableRef::in_schema("main", "users"),
        references: vec!["id".into()],
    });
    let index = SchemaIndex::build(parts);
    let label = |r: &IndexRelation| {
        format!(
            "{}.{} → {}.{} {:?}",
            index.table(r.from as usize).table.name,
            r.columns.join(","),
            index.table(r.to as usize).table.name,
            r.references.join(","),
            r.origin
        )
    };
    let all: Vec<String> = index.relations().iter().map(label).collect();
    assert!(all.contains(&"orders.user_id → users.id Real".to_owned()), "{all:?}");
    assert!(
        all.contains(&"order_items.order_id → orders.id Real".to_owned()),
        "its key filled: {all:?}"
    );
    assert!(all.contains(&"order_items.product_id → products.id Virtual".to_owned()), "{all:?}");
    assert!(all.contains(&"categories.parent_id → categories.id Inferred".to_owned()), "{all:?}");
    assert!(!all.iter().any(|r| r.starts_with("events.user_id")), "set aside: {all:?}");
    // The surest first; each table's both ways.
    assert!(index.relations().windows(2).all(|w| w[0].origin <= w[1].origin));
    let orders = index.find(&TableRef::new("orders")).unwrap();
    assert_eq!(index.relations_of(orders).count(), 2);
}

#[test]
fn a_huge_schema_s_columns_come_when_named() {
    let index = SchemaIndex::build(shop(false));
    assert!(!index.is_complete());
    let users = index.find(&TableRef::new("users")).unwrap();
    assert!(index.table(users).columns.is_none());
    assert!(index.relations().is_empty());
    let named = [TableRef::new("users"), TableRef::new("orders"), TableRef::new("nope")];
    assert_eq!(index.missing(&named), [TableRef::new("users"), TableRef::new("orders")]);
    let next = index.with_columns(shop_loaded(&["users", "orders"]), 2);
    assert_eq!(next.generation(), 2);
    assert_eq!(index.generation(), 1, "the old one unchanged");
    let users = next.find(&TableRef::new("users")).unwrap();
    assert_eq!(next.table(users).columns.as_ref().unwrap().len(), 3);
    assert!(next.missing(&named).is_empty());
    assert_eq!(next.relations().len(), 1, "the key between the two read");
}

#[test]
fn five_thousand_tables_of_thirty_columns_fit_in_twenty_megabytes() {
    let memory = big_index().memory();
    assert!(memory < 20 * 1024 * 1024, "{} MB", memory / (1024 * 1024));
    assert!(memory > 5 * 1024 * 1024, "measured: {memory}");
}

/// The index of 5 000 tables of 30 columns ([`big`]), built once for the tests using it.
pub(crate) fn big_index() -> &'static SchemaIndex {
    static INDEX: std::sync::OnceLock<SchemaIndex> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| SchemaIndex::build(big(5_000, 30)))
}

/// `tables` tables of `columns` columns each, named as a big schema's are (each table's
/// `<previous>_id` pointing back).
pub(crate) fn big(tables: usize, columns: usize) -> IndexParts {
    let mut infos = Vec::new();
    let mut all = Vec::new();
    let mut keys = Vec::new();
    for t in 0..tables {
        let name = format!("table_{t:05}");
        let table = TableRef::in_schema("public", name);
        infos.push(info(table.clone(), TableKind::Table));
        let mut cols = vec![column("id", "integer", Some(1))];
        for c in 1..columns {
            cols.push(column(
                &format!("column_{c:02}"),
                if c % 3 == 0 { "text" } else { "integer" },
                None,
            ));
        }
        if t > 0 {
            let previous = format!("table_{:05}", t - 1);
            cols.push(column(&format!("{previous}_id"), "integer", None));
            keys.push((
                table.clone(),
                vec![fk(
                    &[&format!("{previous}_id")],
                    TableRef::in_schema("public", previous),
                    &["id"],
                )],
            ));
        }
        all.push((table, cols));
    }
    IndexParts {
        default_schema: Some("public".into()),
        schemas: vec!["public".into()],
        tables: infos,
        columns: all,
        keys,
        complete: true,
        generation: 1,
        ..IndexParts::default()
    }
}

#[test]
fn what_may_be_read_from_besides_its_tables() {
    let mut parts = shop(true);
    parts.readable = vec![TableRef::in_schema("main", "Orders_2024")];
    let index = SchemaIndex::build(parts);
    assert!(index.exists(&TableRef::new("users")));
    assert!(index.exists(&TableRef::new("orders_2024")), "another relation, any case");
    assert!(index.exists(&TableRef::in_schema("main", "orders_2024")));
    assert!(!index.exists(&TableRef::in_schema("other", "orders_2024")));
    assert!(!index.exists(&TableRef::new("nope")));
    assert!(index.knows_schema("MAIN") && !index.knows_schema("temp"));
}

#[test]
fn a_column_and_what_it_points_to() {
    let index = SchemaIndex::build(shop(true));
    let orders = index.find(&TableRef::new("orders")).unwrap();
    let users = index.find(&TableRef::new("users")).unwrap();
    assert_eq!(index.column(orders, "USER_ID"), Some(1));
    assert_eq!(index.column(orders, "nope"), None);
    assert_eq!(index.points_to(orders, "user_id"), Some((users, "id".to_owned())));
    assert_eq!(index.points_to(orders, "total"), None);
    // Not read: no column.
    let lazy = SchemaIndex::build(shop(false));
    let orders = lazy.find(&TableRef::new("orders")).unwrap();
    assert_eq!(lazy.column(orders, "id"), None);
}
