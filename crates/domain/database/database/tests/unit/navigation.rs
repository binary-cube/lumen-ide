use super::*;
use crate::schema_index::tests::shop;
use crate::sql::lex;

const LITE: Flavor = Flavor::Sqlite;

/// What's at `‸` in `sql`, with the shop's schema (`complete`: every column read).
fn at_with(sql: &str, complete: bool) -> (Option<Lookup>, Vec<TableRef>, SchemaIndex, String) {
    let offset = sql.find('‸').expect("a cursor");
    let text = sql.replace('‸', "");
    let index = SchemaIndex::build(shop(complete));
    let (found, missing) = lookup(&text, &lex(&text, LITE), offset, LITE, Some(&index));
    (found, missing, index, text)
}

fn at(sql: &str) -> (Target, SchemaIndex, String) {
    let (found, _, index, text) = at_with(sql, true);
    (found.expect("a name there").target, index, text)
}

fn table_name(index: &SchemaIndex, target: &Target) -> String {
    match target {
        Target::Table { table, .. } | Target::Column { table, .. } => {
            index.table(*table).table.name.clone()
        }
        other => panic!("not a table: {other:?}"),
    }
}

#[test]
fn a_table_named_is_the_schema_s() {
    let (t, index, _) = at("SELECT * FROM ord‸ers o");
    assert_eq!(table_name(&index, &t), "orders");
    assert!(matches!(t, Target::Table { alias: None, .. }));
    // Its alias where it's given: the table too.
    let (t, index, _) = at("SELECT * FROM orders o‸");
    assert_eq!(table_name(&index, &t), "orders");
}

#[test]
fn an_alias_used_leads_to_where_it_s_given() {
    let (t, index, text) = at("SELECT o‸.total FROM orders o");
    assert_eq!(table_name(&index, &t), "orders");
    let Target::Table { alias: Some(alias), .. } = t else { panic!("an alias") };
    assert_eq!(&text[alias.clone()], "o");
    assert_eq!(alias.start, text.rfind('o').unwrap());
}

#[test]
fn a_column_is_its_table_s() {
    let (t, index, _) = at("SELECT o.tot‸al FROM orders o");
    let Target::Column { table, column } = t else { panic!("a column") };
    assert_eq!(index.table(table).table.name, "orders");
    assert_eq!(&*index.table(table).columns.as_ref().unwrap()[column].name, "total");
    // Unqualified, the only table having it.
    let (t, index, _) = at("SELECT email‸ FROM users u JOIN orders o ON o.user_id = u.id");
    assert_eq!(table_name(&index, &t), "users");
    // An insert's list.
    let (t, index, _) = at("INSERT INTO products (pri‸ce) VALUES (1)");
    assert_eq!(table_name(&index, &t), "products");
    // A column of a table not read: nothing (asked for).
    let (found, missing, _, _) = at_with("SELECT o.total‸ FROM orders o", false);
    assert!(found.is_none());
    assert_eq!(missing, [TableRef::new("orders")]);
}

#[test]
fn an_unqualified_column_in_two_tables_is_ambiguous() {
    let (t, _, _) = at("SELECT id‸ FROM users u JOIN orders o ON o.user_id = u.id");
    assert_eq!(t, Target::Ambiguous { column: "id".into(), owners: vec!["u".into(), "o".into()] });
    // A nearer level's table hides the outer ones.
    let (t, index, _) =
        at("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE id‸ = 1)");
    assert_eq!(table_name(&index, &t), "orders");
}

#[test]
fn ctes_and_derived_tables_lead_to_their_definitions() {
    let sql = "WITH recent AS (SELECT id, total FROM orders) SELECT r.tot‸al FROM recent r";
    let (t, _, text) = at(sql);
    let Target::LocalColumn { column, owner, kind, defined } = t else { panic!("{t:?}") };
    assert_eq!((column.as_str(), owner.as_str(), kind), ("total", "r", LocalKind::Cte));
    assert_eq!(&text[defined], "recent");
    // The CTE's use: its definition.
    let (t, _, text) = at("WITH recent AS (SELECT id FROM orders) SELECT * FROM rec‸ent");
    let Target::Local { defined, kind, columns, .. } = t else { panic!("a local table") };
    assert_eq!((kind, columns), (LocalKind::Cte, vec!["id".to_owned()]));
    assert_eq!(defined.start, text.find("recent").unwrap());
    // A derived table's alias: where it's given; its column: its `(`.
    let (t, _, text) = at("SELECT d‸.n FROM (SELECT count(*) AS n FROM users) d");
    let Target::Local { defined, kind, .. } = t else { panic!("a local table") };
    assert_eq!(kind, LocalKind::Derived);
    assert_eq!(&text[defined], "d");
    let (t, _, text) = at("SELECT d.n‸ FROM (SELECT count(*) AS n FROM users) d");
    let Target::LocalColumn { defined, .. } = t else { panic!("a local column") };
    assert_eq!(&text[defined], "(");
}

#[test]
fn nothing_where_no_name_is() {
    let (found, ..) = at_with("SELECT 'or‸ders' FROM orders", true);
    assert!(found.is_none());
    let (found, ..) = at_with("SEL‸ECT 1", true);
    assert!(found.is_none());
    let (found, ..) = at_with("SELECT cou‸nt(*) FROM orders", true);
    assert!(found.is_none(), "a function");
    // A table the schema doesn't have.
    let (found, ..) = at_with("SELECT * FROM nope‸", true);
    assert!(found.is_none());
}
