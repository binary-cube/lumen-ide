use std::time::{Duration, Instant};

use database::diagram::{Cardinality, SchemaGraph};
use database::{ColumnInfo, FkAction, ForeignKey, Structure, TableInfo, TableKind, TableRef};

use super::*;

fn column(name: &str, key: bool) -> ColumnInfo {
    let mut c = ColumnInfo::new(name, "INTEGER");
    c.in_key = key;
    c
}

fn key(column: &str, table: &str) -> ForeignKey {
    ForeignKey {
        name: None,
        columns: vec![column.into()],
        table: TableRef::new(table),
        references: vec!["id".into()],
        on_update: FkAction::NoAction,
        on_delete: FkAction::NoAction,
    }
}

fn table(name: &str, columns: &[&str], keys: &[(&str, &str)]) -> (TableInfo, Structure) {
    let table = TableRef::new(name);
    let mut all = vec![column("id", true)];
    all.extend(columns.iter().map(|c| column(c, false)));
    let structure = Structure {
        table: table.clone(),
        columns: all,
        foreign_keys: keys.iter().map(|(c, t)| key(c, t)).collect(),
        ..Structure::default()
    };
    (TableInfo { table, kind: TableKind::Table, rows: None }, structure)
}

fn shop() -> SchemaGraph {
    SchemaGraph::new(vec![
        table("users", &["name"], &[]),
        table("orders", &["user_id", "total"], &[("user_id", "users")]),
        table(
            "lines",
            &["order_id", "product_id"],
            &[("order_id", "orders"), ("product_id", "products")],
        ),
        table("products", &["title"], &[]),
        table("categories", &["parent_id"], &[("parent_id", "categories")]),
        table("settings", &["value"], &[]),
    ])
}

fn all(graph: &SchemaGraph) -> Shown {
    Shown {
        tables: (0..graph.tables.len()).collect(),
        links: (0..graph.links.len()).collect(),
        focus: None,
    }
}

fn rect(graph: &SchemaGraph, out: &DiagramLayout, name: &str) -> Rect {
    let t = graph.find(&TableRef::new(name)).unwrap();
    out.layout.cards[out.card_of(t).unwrap()].rect
}

#[test]
fn tables_stand_right_of_those_they_point_to() {
    let g = shop();
    let out = lay_out(&g, &all(&g));
    assert_eq!(out.layout.cards.len(), 6);
    let r = |n: &str| rect(&g, &out, n);
    assert!(r("users").max_x() < r("orders").min_x());
    assert!(r("orders").max_x() < r("lines").min_x());
    assert!(r("products").max_x() < r("lines").min_x());
    // Linked to nothing (a tree pointing to itself too): under the rest.
    let linked_bottom = ["users", "orders", "lines", "products"]
        .map(|n| r(n).max_y())
        .into_iter()
        .fold(f32::MIN, f32::max);
    assert!(r("settings").min_y() > linked_bottom);
    assert!(r("categories").min_y() > linked_bottom);
    // No two cards overlap.
    for (i, a) in out.layout.cards.iter().enumerate() {
        for b in &out.layout.cards[i + 1..] {
            assert!(
                a.rect
                    .intersect(&b.rect)
                    .is_none_or(|x| x.size.width <= 0.0 || x.size.height <= 0.0),
                "{a:?} {b:?}"
            );
        }
    }
}

#[test]
fn lines_join_the_columns_rows_and_end_on_the_referenced_table() {
    let g = shop();
    let out = lay_out(&g, &all(&g));
    assert_eq!(out.layout.edges.len(), g.links.len());
    for edge in &out.layout.edges {
        for pair in edge.points.windows(2) {
            assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "{edge:?}");
        }
    }
    let orders = g.find(&TableRef::new("orders")).unwrap();
    let edge = out.layout.edges.iter().find(|e| g.links[e.kind].from == orders).unwrap();
    let (users, orders_rect) = (rect(&g, &out, "users"), rect(&g, &out, "orders"));
    assert_eq!(edge.dir, Dir::Left);
    let (start, end) = (edge.points[0], *edge.points.last().unwrap());
    assert_eq!((start.x, end.x), (orders_rect.min_x(), users.max_x()));
    // From `user_id`'s row (the second listed) to `id`'s (the first).
    assert_eq!(start.y, orders_rect.min_y() + HEADER_H + 4.0 + ROW_H * 1.5);
    assert_eq!(end.y, users.min_y() + HEADER_H + 4.0 + ROW_H * 0.5);
    assert_eq!(g.links[edge.kind].cardinality, Cardinality::OneToMany);
    // A table pointing to itself: a loop on its right.
    let categories = g.find(&TableRef::new("categories")).unwrap();
    let tree = out.layout.edges.iter().find(|e| g.links[e.kind].from == categories).unwrap();
    let r = rect(&g, &out, "categories");
    assert!(tree.points[1].x > r.max_x());
    assert_eq!((tree.points[0].x, tree.points[3].x), (r.max_x(), r.max_x()));
}

#[test]
fn cards_list_their_keys_first_and_count_the_rest() {
    let mut many: Vec<String> = (0..30).map(|i| format!("c{i}")).collect();
    many.push("user_id".into());
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let g = SchemaGraph::new(vec![
        table("users", &[], &[]),
        table("wide", &refs, &[("user_id", "users")]),
    ]);
    let out = lay_out(&g, &all(&g));
    let wide = out.card_of(g.find(&TableRef::new("wide")).unwrap()).unwrap();
    let card = &out.layout.cards[wide];
    assert_eq!((card.shown, card.more), (10, 22));
    let names: Vec<&str> = out.rows[wide]
        .iter()
        .map(|&i| g.tables[card.node].structure.columns[i].name.as_str())
        .collect();
    assert_eq!(names.first(), Some(&"id"));
    assert_eq!(names.last(), Some(&"user_id"), "its link's column, in the table's order");
    // The table the diagram is about lists more.
    let focused = lay_out(&g, &Shown { focus: Some(card.node), ..all(&g) });
    assert_eq!(focused.layout.cards[wide].shown, 18);
}

/// `n` tables, each pointing to one or two earlier ones (and a few to themselves).
fn large(n: usize) -> SchemaGraph {
    let tables = (0..n)
        .map(|i| {
            let name = format!("t{i:03}");
            let mut keys = Vec::new();
            if i > 0 {
                keys.push(("a_id".to_owned(), format!("t{:03}", (i * 7) % i)));
            }
            if i > 3 && i % 3 == 0 {
                keys.push(("b_id".to_owned(), format!("t{:03}", i / 3)));
            }
            if i % 50 == 0 {
                keys.push(("parent_id".to_owned(), name.clone()));
            }
            let keys: Vec<(&str, &str)> =
                keys.iter().map(|(c, t)| (c.as_str(), t.as_str())).collect();
            table(&name, &["a_id", "b_id", "parent_id", "name"], &keys)
        })
        .collect();
    SchemaGraph::new(tables)
}

#[test]
fn five_hundred_tables_lay_out_fast_and_the_same_every_time() {
    let g = large(500);
    let shown = all(&g);
    let started = Instant::now();
    let out = lay_out(&g, &shown);
    let took = started.elapsed();
    // A debug build on a loaded machine: generous, but far from a frame's worth per table.
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert_eq!(out.layout.cards.len(), 500);
    assert_eq!(out.layout.edges.len(), g.links.len());
    assert_eq!(out, lay_out(&g, &shown), "deterministic");
    for (i, a) in out.layout.cards.iter().enumerate() {
        for b in &out.layout.cards[i + 1..] {
            let overlap = a
                .rect
                .intersect(&b.rect)
                .is_some_and(|x| x.size.width > 0.0 && x.size.height > 0.0);
            assert!(!overlap, "{a:?} {b:?}");
        }
    }
    // Not one endless column: wrapped into a readable shape.
    let b = out.layout.bounds;
    assert!(b.size.height < b.size.width * 4.0, "{b:?}");
}

#[test]
fn a_table_and_its_neighbors_lay_out_alone() {
    let g = shop();
    let users = g.find(&TableRef::new("users")).unwrap();
    let tables = g.neighborhood(users, 1, |_| true);
    let links = (0..g.links.len())
        .filter(|&l| tables.contains(&g.links[l].from) && tables.contains(&g.links[l].to))
        .collect();
    let out = lay_out(&g, &Shown { tables, links, focus: Some(users) });
    let names: Vec<&str> =
        out.layout.cards.iter().map(|c| g.tables[c.node].table().name.as_str()).collect();
    assert_eq!(names, ["orders", "users"]);
    assert_eq!(out.layout.edges.len(), 1);
}
