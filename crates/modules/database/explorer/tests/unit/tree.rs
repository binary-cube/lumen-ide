//! The "Database" tree as a model: nothing read until opened (what to read said once),
//! loading and error rows, the schemas, groups and parts with their counts, a filter
//! showing what matches with the way to it, what's open kept as sources come and go, the
//! sources in their folders.

use database::{ColumnInfo, ForeignKey, IndexInfo, SourceColor};

use super::*;

fn words() -> Words {
    Words {
        loading: "Loading…".into(),
        default_database: "default".into(),
        no_databases: "No databases".into(),
        not_configured: "Not configured".into(),
        no_tables: "No tables".into(),
        tables: "tables".into(),
        views: "views".into(),
        columns: "columns".into(),
        indexes: "indexes".into(),
        keys: "foreign keys".into(),
        unique: "unique".into(),
        global: "global".into(),
        read_only: "read-only".into(),
        open_file: "open file".into(),
        rows: |n| format!("{n} rows"),
    }
}

fn source(name: &str) -> TreeSource {
    TreeSource {
        id: DataSourceId::new(),
        name: name.into(),
        driver: "sqlite".into(),
        driver_name: "SQLite".into(),
        icon: "database",
        scope: Some(Scope::Project),
        file: None,
        configured: true,
        read_only: false,
        schema_changes: database::SchemaChanges::ALL,
        schemas: Vec::new(),
        databases: false,
        shown_databases: Vec::new(),
        server: None,
        folder: None,
        color: None,
        production: false,
        transaction: false,
    }
}

fn table(name: &str, kind: TableKind, rows: Option<u64>) -> TableInfo {
    TableInfo { table: TableRef::in_schema("main", name), kind, rows }
}

fn orders() -> Structure {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let mut customer = ColumnInfo::new("customer_id", "INTEGER");
    customer.nullable = false;
    Structure {
        table: TableRef::in_schema("main", "orders"),
        columns: vec![id, customer, ColumnInfo::new("note", "TEXT")],
        indexes: vec![IndexInfo::new("orders_customer", vec!["customer_id".into()], false)],
        foreign_keys: vec![ForeignKey {
            name: None,
            columns: vec!["customer_id".into()],
            table: TableRef::new("customers"),
            references: vec!["id".into()],
            on_update: Default::default(),
            on_delete: Default::default(),
        }],
        rowid: true,
        lines: false,
    }
}

fn labels(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| format!("{}{}", "  ".repeat(r.depth as usize), r.label)).collect()
}

#[test]
fn a_node_is_read_only_once_opened_and_said_once() {
    let mut tree = SourceTree::default();
    let shop = source("shop");
    let id = shop.id;
    tree.set_sources(vec![shop]);
    let all = ListFilter::default();
    assert_eq!(labels(&tree.rows(&all, &words())), ["shop"], "closed: nothing asked");
    assert_eq!(tree.expand(&NodeKey::Source(id)), Some(Need::Tables(id)));
    assert_eq!(tree.expand(&NodeKey::Source(id)), None, "being read: not asked again");
    assert_eq!(labels(&tree.rows(&all, &words())), ["shop", "  Loading…"]);
    tree.set_tables(id, Err("no such file".into()));
    assert_eq!(tree.rows(&all, &words())[1].kind, RowKind::Error);
    // Opened again after a failure: asked again.
    tree.collapse(&NodeKey::Source(id));
    assert_eq!(tree.expand(&NodeKey::Source(id)), Some(Need::Tables(id)));
}

#[test]
fn schemas_groups_tables_and_parts_with_their_counts() {
    let mut tree = SourceTree::default();
    let shop = source("shop");
    let id = shop.id;
    tree.set_sources(vec![shop]);
    tree.expand(&NodeKey::Source(id));
    tree.set_tables(
        id,
        Ok(vec![
            table("orders", TableKind::Table, Some(40)),
            table("big_orders", TableKind::View, None),
            table("customers", TableKind::Table, None),
        ]),
    );
    let rows = tree.rows(&ListFilter::default(), &words());
    assert_eq!(labels(&rows), ["shop", "  main"]);
    assert_eq!((rows[0].count, rows[1].count), (Some(3), Some(3)));
    tree.expand(&NodeKey::Schema(id, "main".into()));
    tree.expand(&NodeKey::Group(id, Some("main".into()), Group::Tables));
    tree.expand(&NodeKey::Group(id, Some("main".into()), Group::Views));
    let orders_key = NodeKey::Table(id, TableRef::in_schema("main", "orders"));
    let need = tree.expand(&orders_key);
    assert_eq!(need, Some(Need::Structure(id, TableRef::in_schema("main", "orders"))));
    tree.set_structure(id, TableRef::in_schema("main", "orders"), Ok(orders()));
    tree.expand(&NodeKey::Part(id, TableRef::in_schema("main", "orders"), Part::Columns));
    let rows = tree.rows(&ListFilter::default(), &words());
    assert_eq!(
        labels(&rows),
        [
            "shop",
            "  main",
            "    tables",
            "      customers",
            "      orders",
            "        columns",
            "          id",
            "          customer_id",
            "          note",
            "        indexes",
            "        foreign keys",
            "    views",
            "      big_orders",
        ]
    );
    let count = |label: &str| rows.iter().find(|r| r.label.trim() == label).and_then(|r| r.count);
    assert_eq!((count("tables"), count("views")), (Some(2), Some(1)));
    assert_eq!((count("orders"), count("columns"), count("indexes")), (Some(3), Some(3), Some(1)));
    let orders = rows.iter().find(|r| r.label == "orders").unwrap();
    assert_eq!(orders.detail, "40 rows", "the estimate, when the engine keeps one");
    let column = |name: &str| rows.iter().find(|r| r.label == name).unwrap();
    assert_eq!(column("id").kind, RowKind::Column { key: true, foreign: false });
    assert_eq!(column("customer_id").kind, RowKind::Column { key: false, foreign: true });
    assert_eq!(column("customer_id").detail, "INTEGER · NOT NULL");
}

#[test]
fn a_filter_shows_what_matches_and_the_way_to_it() {
    let mut tree = SourceTree::default();
    let (shop, blog) = (source("shop"), source("blog"));
    let (id, other) = (shop.id, blog.id);
    tree.set_sources(vec![shop, blog]);
    tree.expand(&NodeKey::Source(id));
    tree.set_tables(
        id,
        Ok(vec![
            table("orders", TableKind::Table, None),
            table("customers", TableKind::Table, None),
        ]),
    );
    tree.set_structure(id, TableRef::in_schema("main", "orders"), Ok(orders()));
    // Nothing of `blog` is read: it shows only if its name matches.
    let rows = tree.rows(&ListFilter::new("cust"), &words());
    assert_eq!(
        labels(&rows),
        [
            "shop",
            "  main",
            "    tables",
            "      customers",
            "      orders",
            "        columns",
            "          customer_id",
            "        indexes",
            "          orders_customer",
            // A key without a name goes by its columns.
            "        foreign keys",
            "          customer_id",
        ]
    );
    let rows = tree.rows(&ListFilter::new("BLO"), &words());
    assert_eq!(labels(&rows), ["blog"]);
    assert_eq!(rows[0].key, NodeKey::Source(other));
    assert!(tree.rows(&ListFilter::new("nothing"), &words()).is_empty());
}

#[test]
fn a_source_not_configured_is_never_connected() {
    let mut tree = SourceTree::default();
    let mut prod = source("prod");
    prod.configured = false;
    let id = prod.id;
    tree.set_sources(vec![prod]);
    assert_eq!(tree.expand(&NodeKey::Source(id)), None);
    let rows = tree.rows(&ListFilter::default(), &words());
    assert_eq!((rows[1].kind.clone(), rows[1].label.as_str()), (RowKind::Hint, "Not configured"));
}

#[test]
fn what_is_open_stays_open_while_sources_come_and_go() {
    let mut tree = SourceTree::default();
    let (shop, blog) = (source("shop"), source("blog"));
    let id = shop.id;
    tree.set_sources(vec![shop.clone(), blog]);
    tree.expand(&NodeKey::Source(id));
    tree.set_tables(id, Ok(vec![table("orders", TableKind::Table, None)]));
    // Another source removed: this one keeps what it read and what's open.
    tree.set_sources(vec![shop.clone()]);
    assert!(tree.is_expanded(&NodeKey::Source(id)));
    assert!(tree.tables(id).is_some());
    // Its settings changed (another file): read again.
    let mut moved = shop;
    moved.file = Some("/elsewhere.sqlite".into());
    tree.set_sources(vec![moved]);
    assert!(tree.tables(id).is_none());
    // A refresh asks again for what's open, the tables first.
    tree.set_tables(id, Ok(vec![table("orders", TableKind::Table, None)]));
    tree.expand(&NodeKey::Table(id, TableRef::in_schema("main", "orders")));
    let needs = tree.refresh(id);
    assert_eq!(needs[0], Need::Tables(id));
    assert_eq!(needs[1], Need::Structure(id, TableRef::in_schema("main", "orders")));
}

#[test]
fn the_selection_falls_back_to_the_nearest_row_shown() {
    let id = DataSourceId::new();
    let table = TableRef::new("orders");
    let row = |key: NodeKey| Row {
        key,
        depth: 0,
        kind: RowKind::Source,
        label: String::new(),
        detail: String::new(),
        count: None,
        expandable: true,
        expanded: false,
        color: None,
        marks: RowMarks::default(),
    };
    let rows = vec![row(NodeKey::Source(id)), row(NodeKey::Table(id, table.clone()))];
    assert_eq!(nearest(&rows, &NodeKey::Column(id, table, "x".into())), Some(1));
    assert_eq!(nearest(&rows, &NodeKey::Schema(id, "main".into())), Some(0));
    assert_eq!(nearest(&rows, &NodeKey::Source(DataSourceId::new())), None);
}

#[test]
fn sources_are_shown_in_their_folders_with_their_colors() {
    let mut tree = SourceTree::default();
    let mut prod = SourceFolder::new("prod", Scope::Project);
    prod.color = Some(SourceColor::Red);
    let mut eu = SourceFolder::new("eu", Scope::Project);
    eu.parent = Some(prod.id);
    let dev = SourceFolder::new("dev", Scope::Project);
    let mut shop = source("shop");
    shop.folder = Some(eu.id);
    shop.color = Some(SourceColor::Red);
    let mut people = source("people");
    people.folder = Some(dev.id);
    let loose = source("loose");
    let mut global = source("analytics");
    global.scope = Some(Scope::Global);
    let (shop_id, people_id) = (shop.id, people.id);
    tree.set_sources(vec![shop, people, loose, global]);
    tree.set_folders(vec![prod.clone(), eu.clone(), dev]);
    let all = ListFilter::default();
    let rows = tree.rows(&all, &words());
    assert_eq!(
        labels(&rows),
        ["dev", "  people", "prod", "  eu", "    shop", "loose", "analytics"]
    );
    assert_eq!(rows[2].kind, RowKind::Folder);
    assert_eq!(rows[2].count, Some(1), "the sources inside, all the way down");
    assert_eq!(rows[2].color, Some(SourceColor::Red));
    assert_eq!(rows[3].color, Some(SourceColor::Red), "a subfolder takes its parent's");
    assert_eq!(rows[4].color, Some(SourceColor::Red));
    assert_eq!(rows[0].color, None);
    // Closed: its insides go; a source revealed opens the folders it's in.
    tree.toggle(&NodeKey::Folder(prod.id));
    assert_eq!(
        labels(&tree.rows(&all, &words())),
        ["dev", "  people", "prod", "loose", "analytics"]
    );
    tree.expand(&NodeKey::Source(shop_id));
    assert!(tree.is_expanded(&NodeKey::Folder(prod.id)));
    assert!(tree.is_expanded(&NodeKey::Folder(eu.id)));
    tree.collapse_all();
    assert_eq!(labels(&tree.rows(&all, &words())), ["dev", "prod", "loose", "analytics"]);
    // The filter matches a folder's name: all it holds is shown.
    let rows = tree.rows(&ListFilter::new("dev"), &words());
    assert_eq!(labels(&rows), ["dev", "  people"]);
    assert_eq!(rows[1].key, NodeKey::Source(people_id));
    let rows = tree.rows(&ListFilter::new("shop"), &words());
    assert_eq!(labels(&rows), ["prod", "  eu", "    shop"]);
    // A source moved or colored isn't read again (only where it's shown changed).
    let mut moved = tree.sources().to_vec();
    moved[1].folder = None;
    moved[1].color = Some(SourceColor::Blue);
    tree.set_tables(people_id, Ok(Vec::new()));
    tree.set_sources(moved);
    assert!(tree.tables(people_id).is_some());
}

/// Production (a folder's, inherited by those inside; a source's) and a console's open
/// transaction are the rows' badges; neither makes a source be read again.
#[test]
fn production_and_a_console_s_transaction_are_marked() {
    let mut tree = SourceTree::default();
    let mut prod = SourceFolder::new("prod", Scope::Project);
    prod.production = true;
    let mut eu = SourceFolder::new("eu", Scope::Project);
    eu.parent = Some(prod.id);
    let mut shop = source("shop");
    shop.folder = Some(eu.id);
    shop.production = true;
    shop.transaction = true;
    let loose = source("loose");
    tree.set_sources(vec![shop.clone(), loose]);
    tree.set_folders(vec![prod, eu]);
    let rows = tree.rows(&ListFilter::default(), &words());
    assert_eq!(labels(&rows), ["prod", "  eu", "    shop", "loose"]);
    assert!(rows[0].marks.production && rows[1].marks.production, "a subfolder too");
    assert_eq!(rows[2].marks, RowMarks { production: true, transaction: true });
    assert_eq!(rows[3].marks, RowMarks::default());
    // Its transaction ended: the same source, nothing read again.
    tree.set_tables(shop.id, Ok(Vec::new()));
    let mut ended = shop;
    ended.transaction = false;
    tree.set_sources(vec![ended]);
    assert!(!tree.rows(&ListFilter::default(), &words()).iter().any(|r| r.marks.transaction));
}

/// A server with several databases: source › database › schema; its databases read first,
/// each one's tables by its own identity, kept as the sources come again, read again with
/// their source.
#[test]
fn a_servers_databases_sit_between_it_and_its_schemas() {
    let mut tree = SourceTree::default();
    let mut pg = source("pg");
    pg.databases = true;
    let id = pg.id;
    tree.set_sources(vec![pg.clone()]);
    let all = ListFilter::default();
    assert_eq!(tree.expand(&NodeKey::Source(id)), Some(Need::Databases(id)));
    assert_eq!(labels(&tree.rows(&all, &words())), ["pg", "  Loading…"]);
    let shop = id.of_database("shop");
    let databases = vec![
        TreeDatabase { name: "shop".into(), id: shop, default: false },
        TreeDatabase { name: "postgres".into(), id, default: true },
    ];
    tree.set_databases(id, Ok(databases.clone()));
    let rows = tree.rows(&all, &words());
    assert_eq!(labels(&rows), ["pg", "  postgres", "  shop"], "by name");
    assert_eq!((rows[0].count, rows[1].detail.as_str()), (Some(2), "default"));
    // A database's own source: its identity, its name with the database's.
    assert_eq!(tree.source(shop).map(|s| s.name.as_str()), Some("pg · shop"));
    assert_eq!((tree.base_of(shop), tree.base_of(id)), (id, id));
    // Opened: its tables, by its identity (the source's own for its database).
    assert_eq!(tree.expand(&NodeKey::Database(shop)), Some(Need::Tables(shop)));
    assert_eq!(tree.expand(&NodeKey::Database(id)), Some(Need::Tables(id)));
    tree.set_tables(shop, Ok(vec![table("orders", TableKind::Table, None)]));
    tree.set_tables(id, Ok(Vec::new()));
    assert_eq!(
        labels(&tree.rows(&all, &words())),
        ["pg", "  postgres", "    No tables", "  shop", "    main"]
    );
    // The sources again (nothing changed): what's read of its databases kept.
    tree.set_sources(vec![pg.clone()]);
    assert!(tree.tables(shop).is_some());
    // Read again: its databases first, then the open ones' tables.
    let needs = tree.refresh(id);
    assert_eq!(needs[0], Need::Databases(id));
    assert!(needs.contains(&Need::Tables(shop)) && needs.contains(&Need::Tables(id)));
    tree.set_databases(id, Ok(databases));
    // A hidden read given up is asked again.
    tree.forget_pending();
    let needs = tree.open_needs();
    assert!(needs.contains(&Need::Tables(shop)), "{needs:?}");
    // Its settings changed: what's known of its databases forgotten.
    tree.set_tables(shop, Ok(vec![table("orders", TableKind::Table, None)]));
    pg.shown_databases = vec!["shop".into()];
    tree.set_sources(vec![pg]);
    assert!(tree.tables(shop).is_none() && tree.databases(id).is_none());
}

/// A database named like the filter shows as without one: closed until opened (opening it
/// reads its tables), then all its tables, even those the filter doesn't name.
#[test]
fn a_database_named_like_the_filter_lists_its_tables() {
    let mut tree = SourceTree::default();
    let mut pg = source("pg");
    pg.databases = true;
    let id = pg.id;
    tree.set_sources(vec![pg]);
    tree.expand(&NodeKey::Source(id));
    let shop = id.of_database("newshop");
    tree.set_databases(
        id,
        Ok(vec![
            TreeDatabase { name: "newshop".into(), id: shop, default: false },
            TreeDatabase { name: "postgres".into(), id, default: true },
        ]),
    );
    let filter = ListFilter::new("new");
    let rows = tree.rows(&filter, &words());
    assert_eq!(labels(&rows), ["pg", "  newshop"]);
    assert!(!rows[1].expanded, "not read: closed, its chevron opens it");
    assert_eq!(tree.expand(&NodeKey::Database(shop)), Some(Need::Tables(shop)));
    assert_eq!(labels(&tree.rows(&filter, &words())), ["pg", "  newshop", "    Loading…"]);
    tree.set_tables(shop, Ok(vec![table("orders", TableKind::Table, None)]));
    tree.expand(&NodeKey::Schema(shop, "main".into()));
    tree.expand(&NodeKey::Group(shop, Some("main".into()), Group::Tables));
    assert_eq!(
        labels(&tree.rows(&filter, &words())),
        ["pg", "  newshop", "    main", "      tables", "        orders"],
        "its tables, though the filter names none"
    );
}
