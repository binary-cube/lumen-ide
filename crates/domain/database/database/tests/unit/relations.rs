use std::time::{Duration, Instant};

use super::*;
use crate::diagram::{Cardinality, Origin};
use crate::{FkAction, ForeignKey, Structure, TableInfo};

fn column(name: &str, ty: &str) -> ColumnInfo {
    ColumnInfo::new(name, ty)
}

fn key_column(name: &str, ty: &str) -> ColumnInfo {
    let mut c = ColumnInfo::new(name, ty);
    c.in_key = true;
    c
}

/// A table with `id INTEGER` as its key and `columns`.
fn table(name: &str, columns: &[(&str, &str)]) -> (TableInfo, Structure) {
    let mut all = vec![key_column("id", "INTEGER")];
    all.extend(columns.iter().map(|(n, t)| column(n, t)));
    with(name, all)
}

fn with(name: &str, columns: Vec<ColumnInfo>) -> (TableInfo, Structure) {
    let table = TableRef::new(name);
    let info = TableInfo { table: table.clone(), kind: TableKind::Table, rows: None };
    (info, Structure { table, columns, ..Structure::default() })
}

/// What `graph` infers, as `table.column → target.key (confidence)`.
fn found(graph: &SchemaGraph) -> Vec<String> {
    infer(graph)
        .iter()
        .map(|c| {
            format!(
                "{}.{} → {}.{} ({:?})",
                c.key.table.name,
                c.key.columns[0],
                c.key.target.name,
                c.key.references[0],
                c.confidence
            )
        })
        .collect()
}

fn graph(tables: Vec<(TableInfo, Structure)>) -> SchemaGraph {
    SchemaGraph::new(tables)
}

#[test]
fn a_column_is_split_into_its_words() {
    let cases: [(&str, &[&str]); 8] = [
        ("user_id", &["user", "id"]),
        ("userId", &["user", "id"]),
        ("UserID", &["user", "id"]),
        ("userid", &["userid"]),
        ("HTTPServer", &["http", "server"]),
        ("order-item id", &["order", "item", "id"]),
        ("address2_id", &["address", "2", "id"]),
        ("ID", &["id"]),
    ];
    for (column, expected) in cases {
        assert_eq!(words(column), expected, "{column}");
    }
}

#[test]
fn every_form_of_a_key_column_names_its_stem() {
    for column in [
        "userId",
        "user_id",
        "userid",
        "UserID",
        "user_fk",
        "fk_user",
        "id_user",
        "USER_ID",
        "fk_user_id",
    ] {
        let stem = stem(column).unwrap_or_else(|| panic!("{column}"));
        assert_eq!(stem.words, ["user"], "{column}");
    }
    assert!(stem("user_id").unwrap().usual);
    assert!(stem("userId").unwrap().usual);
    assert!(!stem("fk_user").unwrap().usual);
    assert!(!stem("id_user").unwrap().usual);
    assert!(stem("userid").unwrap().fused);
    assert_eq!(stem("order_item_id").unwrap().words, ["order", "item"]);
    for column in ["id", "name", "created_at", "user_name", "ID", "idx"] {
        assert!(stem(column).is_none(), "{column}");
    }
}

#[test]
fn words_are_made_singular_regular_or_not() {
    let cases = [
        ("users", "user"),
        ("categories", "category"),
        ("boxes", "box"),
        ("statuses", "status"),
        ("people", "person"),
        ("children", "child"),
        ("addresses", "address"),
        ("status", "status"),
        ("address", "address"),
        ("analysis", "analysis"),
        ("indices", "index"),
        ("movies", "movie"),
        ("series", "series"),
        ("data", "data"),
        ("person", "person"),
    ];
    for (many, one) in cases {
        assert_eq!(singular(many), one, "{many}");
    }
    // Both readings of `-ses`: `houses` is a `house`.
    assert!(singulars("houses").contains(&"house".to_owned()));
}

#[test]
fn every_form_finds_the_table_singular_or_plural_with_a_prefix_or_not() {
    for target in ["user", "users", "User", "Users", "tbl_user", "t_user", "tbl_users"] {
        for column in ["userId", "user_id", "userid", "UserID", "user_fk", "fk_user", "id_user"] {
            let g = graph(vec![table(target, &[]), table("orders", &[(column, "INTEGER")])]);
            let found = infer(&g);
            assert_eq!(found.len(), 1, "{column} → {target}");
            assert_eq!(found[0].key.target.name, target);
            assert_eq!(found[0].key.references, ["id"]);
        }
    }
}

#[test]
fn plurals_regular_and_irregular() {
    let g = graph(vec![
        table("categories", &[]),
        table("boxes", &[]),
        table("people", &[]),
        table("statuses", &[]),
        table(
            "items",
            &[
                ("category_id", "INTEGER"),
                ("box_id", "INTEGER"),
                ("person_id", "INTEGER"),
                ("status_id", "INTEGER"),
            ],
        ),
    ]);
    assert_eq!(
        found(&g),
        [
            "items.category_id → categories.id (High)",
            "items.box_id → boxes.id (High)",
            "items.person_id → people.id (High)",
            "items.status_id → statuses.id (High)",
        ]
    );
}

#[test]
fn nothing_is_found_without_a_table_so_named() {
    let g = graph(vec![
        table("users", &[]),
        table(
            "orders",
            &[
                ("status_id", "INTEGER"),
                ("paid", "INTEGER"),
                ("valid", "INTEGER"),
                ("guid", "TEXT"),
                ("uuid", "TEXT"),
                ("void", "INTEGER"),
                ("user_name", "TEXT"),
            ],
        ),
    ]);
    assert!(found(&g).is_empty(), "{:?}", found(&g));
}

#[test]
fn types_must_agree() {
    let mut text_key = table("codes", &[]);
    text_key.1.columns[0] = key_column("id", "TEXT");
    let g = graph(vec![
        table("users", &[]),
        text_key,
        table(
            "orders",
            &[
                // Text never holds an integer key, nor an integer a text one.
                ("user_id", "VARCHAR(20)"),
                ("code_id", "INTEGER"),
            ],
        ),
        table("notes", &[("user_id", "BIGINT"), ("code_id", "uuid")]),
        // SQLite: a column without a type holds anything (less sure).
        table("events", &[("user_id", "")]),
    ]);
    assert_eq!(
        found(&g),
        [
            "events.user_id → users.id (Medium)",
            "notes.user_id → users.id (High)",
            "notes.code_id → codes.id (High)",
        ]
    );
}

#[test]
fn the_key_is_id_or_the_primary_key_of_one_column() {
    let mut sku = with("products", vec![key_column("sku", "TEXT"), column("title", "TEXT")]);
    sku.1.columns[0].key_position = Some(1);
    let pair = with("pairs", vec![key_column("a", "INTEGER"), key_column("b", "INTEGER")]);
    let g =
        graph(vec![sku, pair, table("lines", &[("product_id", "TEXT"), ("pair_id", "INTEGER")])]);
    // Not `id`: less sure. A composite key: nothing to point to.
    assert_eq!(found(&g), ["lines.product_id → products.sku (Medium)"]);
}

#[test]
fn a_tree_points_to_its_own_table_unless_a_table_bears_the_name() {
    let g = graph(vec![
        table("categories", &[("parent_id", "INTEGER")]),
        table("employees", &[("managerId", "INTEGER"), ("supervisor_id", "INTEGER")]),
        table("comments", &[("parent_id", "INTEGER")]),
        table("parents", &[]),
    ]);
    assert_eq!(
        found(&g),
        [
            "categories.parent_id → parents.id (High)",
            "comments.parent_id → parents.id (High)",
            "employees.managerId → employees.id (Medium)",
            "employees.supervisor_id → employees.id (Medium)",
        ]
    );
    let g = graph(vec![table("categories", &[("parent_id", "INTEGER")])]);
    assert_eq!(found(&g), ["categories.parent_id → categories.id (High)"]);
}

#[test]
fn a_real_key_is_never_found_again_nor_a_tables_own_key() {
    let mut orders = table("orders", &[("user_id", "INTEGER")]);
    orders.1.foreign_keys.push(ForeignKey {
        name: None,
        columns: vec!["user_id".into()],
        table: TableRef::new("users"),
        references: vec!["id".into()],
        on_update: FkAction::NoAction,
        on_delete: FkAction::NoAction,
    });
    // `users.user_id` its own key: never pointing to itself.
    let users = with("users", vec![key_column("user_id", "INTEGER")]);
    // A table extending another by its key (one to one): found.
    let profiles = with("profiles", vec![key_column("user_id", "INTEGER"), column("bio", "TEXT")]);
    let g = graph(vec![users, orders, profiles]);
    let found = infer(&g);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].key.table.name, "profiles");
    assert_eq!(found[0].key.references, ["user_id"]);
}

#[test]
fn confidence_orders_the_forms() {
    let g = graph(vec![
        table("users", &[]),
        table("tbl_orders", &[]),
        table("a", &[("user_id", "INTEGER")]),
        table("b", &[("fk_user", "INTEGER")]),
        table("c", &[("userid", "INTEGER")]),
        table("d", &[("order_id", "INTEGER")]),
    ]);
    let by = |t: &str| infer(&g).into_iter().find(|c| c.key.table.name == t).unwrap().confidence;
    assert_eq!(by("a"), Confidence::High);
    assert_eq!(by("b"), Confidence::Medium);
    assert_eq!(by("c"), Confidence::Medium);
    assert_eq!(by("d"), Confidence::Medium, "a prefixed table");
    assert!(Confidence::High > Confidence::Medium && Confidence::Medium > Confidence::Low);
}

#[test]
// The test measures how long it takes (the domain itself never reads the clock).
#[allow(clippy::disallowed_methods)]
fn the_same_schema_gives_the_same_relations_quickly() {
    let tables: Vec<_> = (0..1500)
        .map(|i| {
            let columns: Vec<(String, &str)> = vec![
                (format!("t{}_item_id", (i * 7) % 1500), "INTEGER"),
                (format!("item{}Id", i % 40), "INTEGER"),
                ("name".to_owned(), "TEXT"),
                ("paid".to_owned(), "INTEGER"),
            ];
            let columns: Vec<(&str, &str)> =
                columns.iter().map(|(n, t)| (n.as_str(), *t)).collect();
            table(&format!("t{i}_items"), &columns)
        })
        .collect();
    let g = graph(tables);
    let started = Instant::now();
    let first = infer(&g);
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    // Each table points to another, but the six whose number comes back to their own.
    assert_eq!(first.len(), 1494);
    assert_eq!(first, infer(&g), "deterministic");
}

#[test]
fn decisions_make_virtual_links_and_hide_what_was_set_aside() {
    let base = graph(vec![
        table("users", &[]),
        table("products", &[]),
        table("reviews", &[("user_id", "INTEGER"), ("product_id", "INTEGER")]),
    ]);
    let candidates = infer(&base);
    assert_eq!(candidates.len(), 2);
    let mut notes = RelationNotes::default();
    let g = base.with_notes(&candidates, &notes);
    assert!(g.links.iter().all(|l| matches!(l.origin, Origin::Inferred(Confidence::High))));
    notes.accept(candidates[0].key.clone());
    notes.ignore(candidates[1].key.clone());
    let g = base.with_notes(&candidates, &notes);
    assert_eq!(g.links.len(), 1);
    assert_eq!(g.links[0].origin, Origin::Virtual);
    assert_eq!(g.links[0].cardinality, Cardinality::OneToMany);
    assert_eq!(g.label(&g.links[0]), "reviews.user_id → users.id");
    // Undone: found again.
    notes.forget(&candidates[1].key);
    let g = base.with_notes(&candidates, &notes);
    assert_eq!(g.links.len(), 2);
    // Accepting what was set aside moves it.
    notes.ignore(candidates[0].key.clone());
    assert!(notes.virtual_keys.is_empty() && notes.is_ignored(&candidates[0].key));
    // A key compared without case.
    let mut upper = candidates[0].key.clone();
    upper.columns = vec!["USER_ID".into()];
    assert!(notes.is_ignored(&upper));
    // Shown by kind.
    assert!(
        Origin::Virtual.shown(RelationKinds::Real)
            && Origin::Virtual.shown(RelationKinds::Inferred)
    );
    assert!(!Origin::Inferred(Confidence::High).shown(RelationKinds::Real));
    assert!(!Origin::Real.shown(RelationKinds::Inferred));
    assert_eq!(RelationKinds::parse(RelationKinds::Inferred.name()), Some(RelationKinds::Inferred));
}
