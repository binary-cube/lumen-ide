//! The layouts kept: the newest last, one per source and table, at most `KEPT`; files
//! named relative to the project.

use std::path::Path;

use super::*;

fn key(source: &str, table: &str) -> LayoutKey {
    LayoutKey { source: source.into(), table: TableRef::new(table) }
}

#[test]
fn a_layout_kept_again_moves_last_and_the_oldest_go() {
    let mut all = Vec::new();
    keep(&mut all, key("a.db", "t"), GridLayout::default());
    keep(&mut all, key("b.db", "t"), GridLayout::default());
    let hidden = GridLayout { hidden: vec!["x".into()], ..GridLayout::default() };
    keep(&mut all, key("a.db", "t"), hidden.clone());
    assert_eq!(all.len(), 2);
    assert_eq!(all[1], (key("a.db", "t"), hidden));
    for i in 0..KEPT + 5 {
        keep(&mut all, key("c.db", &format!("t{i}")), GridLayout::default());
    }
    assert_eq!(all.len(), KEPT);
    assert_eq!(all.last().map(|(k, _)| k.table.name.clone()), Some(format!("t{}", KEPT + 4)));
}

#[test]
fn files_are_named_from_the_project_root() {
    let root = Path::new("/p");
    assert_eq!(file_source(root, &root.join("data").join("a.db")), Some("data/a.db".into()));
    assert_eq!(file_source(root, Path::new("/elsewhere/a.db")), None);
}

#[test]
fn layouts_follow_columns_and_tables_renamed_or_dropped() {
    use crate::{ColumnDef, SchemaChange};
    let layout = GridLayout {
        widths: vec![("age".into(), 80.0), ("name".into(), 120.0)],
        order: vec!["name".into(), "age".into()],
        hidden: vec!["age".into()],
        sort: vec![Order { column: "age".into(), descending: true }],
        filter: Some("age > 3".into()),
        page: None,
    };
    let mut all = vec![(key("a.db", "t"), layout.clone()), (key("b.db", "t"), layout.clone())];
    let rename = SchemaChange::AlterColumn {
        table: TableRef::new("t"),
        column: "age".into(),
        to: ColumnDef::new("years", "integer"),
    };
    assert!(follow(&mut all, "a.db", &[rename]));
    let followed = &all[0].1;
    assert_eq!(followed.widths[0].0, "years");
    assert_eq!(followed.order, vec!["name".to_owned(), "years".to_owned()]);
    assert_eq!(followed.hidden, vec!["years".to_owned()]);
    assert_eq!(followed.sort[0].column, "years");
    assert_eq!(followed.filter.as_deref(), Some("years > 3"));
    assert_eq!(all[1].1, layout, "another source's untouched");

    let drop = SchemaChange::DropColumn { table: TableRef::new("t"), column: "years".into() };
    follow(&mut all, "a.db", &[drop]);
    let dropped = &all[0].1;
    assert_eq!(dropped.widths.len(), 1);
    assert!(dropped.hidden.is_empty() && dropped.sort.is_empty() && dropped.filter.is_none());

    let renamed = SchemaChange::RenameTable { table: TableRef::new("t"), to: "u".into() };
    follow(&mut all, "b.db", &[renamed]);
    assert_eq!(all[1].0, key("b.db", "u"));
    let gone = SchemaChange::DropTable { table: TableRef::new("u") };
    follow(&mut all, "b.db", &[gone]);
    assert_eq!(all.len(), 1);
    assert!(!follow(&mut all, "c.db", &[SchemaChange::DropTable { table: TableRef::new("t") }]));
}
