//! The forms as state: what each becomes (the changes the domain plans), and why it can't
//! yet.

use super::*;
use database::sqlite::Sqlite;
use database::{ColumnInfo, DbError, SchemaChanges};

fn orders() -> Structure {
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let mut status = ColumnInfo::new("status", "TEXT");
    status.default = Some("'new'".into());
    Structure {
        table: TableRef::new("orders"),
        columns: vec![id, ColumnInfo::new("total", "REAL"), status],
        indexes: vec![IndexInfo::new("orders_total", vec!["total".into()], false)],
        rowid: true,
        ..Structure::default()
    }
}

fn ctx(table: Option<&str>) -> Context {
    let info =
        |name: &str| TableInfo { table: TableRef::new(name), kind: TableKind::Table, rows: None };
    Context {
        table: table.map(TableRef::new),
        schema: None,
        structure: table.map(|_| orders()),
        tables: vec![info("orders"), info("customers")],
        rows: Vec::new(),
        can: SchemaChanges::ALL,
        dialect: Some(Arc::new(Sqlite)),
    }
}

fn csv() -> Context {
    Context {
        dialect: None,
        can: SchemaChanges {
            columns: true,
            renames: true,
            reorder: true,
            ..SchemaChanges::default()
        },
        ..ctx(Some("orders"))
    }
}

#[test]
fn a_new_table_starts_with_a_numbered_key_and_grows_a_spare_row() {
    let mut form = Form::new(Kind::CreateTable, ctx(None));
    assert_eq!(form.columns.len(), 2);
    assert!(form.columns[0].auto && form.columns[0].primary);
    assert!(form.changes().is_err(), "no name yet");
    form.table_name.set_text("orders");
    assert!(form.changes().is_err(), "a name taken");
    form.table_name.set_text("invoices");
    form.columns[1].name.set_text("total");
    form.edited(Text::Name(1));
    assert_eq!(form.columns.len(), 3, "a spare row after the last one typed in");
    form.columns[1].ty.set_text("NUMERIC");
    form.columns[1].default.set_text("0");
    form.columns[1].unique = true;
    form.columns[1].check.set_text("total >= 0");
    let changes = form.changes().unwrap();
    let [SchemaChange::CreateTable(def)] = changes.as_slice() else { panic!("{changes:?}") };
    assert_eq!(def.table, TableRef::new("invoices"));
    assert_eq!(def.columns.len(), 2, "the blank row left out");
    assert_eq!(def.primary_key, vec!["id".to_owned()]);
    assert_eq!(def.uniques, vec![vec!["total".to_owned()]]);
    assert_eq!(def.checks, vec!["total >= 0".to_owned()]);
    assert_eq!(def.columns[1].default, Some(DefaultValue::Value(Value::Int(0))));
    // A composite key; two columns of one name refused.
    form.columns[0].auto = false;
    form.columns[1].primary = true;
    let changes = form.changes().unwrap();
    let [SchemaChange::CreateTable(def)] = changes.as_slice() else { panic!() };
    assert_eq!(def.primary_key, vec!["id".to_owned(), "total".to_owned()]);
    form.columns[1].name.set_text("ID");
    assert!(form.changes().is_err());
}

#[test]
fn what_the_engine_refuses_is_said_before_it_runs() {
    let mut form = Form::new(Kind::CreateTable, ctx(None));
    form.table_name.set_text("t");
    // SQLite numbers only an INTEGER key.
    form.columns[0].ty.set_text("TEXT");
    let why = form.changes().unwrap_err();
    assert!(!why.is_empty());
    form.columns[0].ty.set_text("INTEGER); DROP TABLE x");
    assert!(form.changes().is_err(), "a type that isn't one");
    form.columns[0].ty.set_text("INTEGER");
    form.without_rowid = true;
    form.columns[0].auto = false;
    form.columns[0].primary = false;
    assert!(form.changes().is_err(), "WITHOUT ROWID needs a key");
}

#[test]
fn a_foreign_key_needs_its_target_and_as_many_columns() {
    let mut form = Form::new(Kind::CreateTable, ctx(None));
    form.table_name.set_text("invoices");
    form.columns[1].name.set_text("customer_id");
    form.columns[1].ty.set_text("INTEGER");
    form.keys.push(KeyRow { columns: vec!["customer_id".into()], ..KeyRow::default() });
    assert!(form.changes().is_err(), "no target");
    form.keys[0].target = Some(TableRef::new("customers"));
    form.keys[0].references = vec!["id".into(), "name".into()];
    assert!(form.changes().is_err(), "1 column → 2");
    form.keys[0].references = vec!["id".into()];
    form.keys[0].on_delete = FkAction::Cascade;
    let changes = form.changes().unwrap();
    let [SchemaChange::CreateTable(def)] = changes.as_slice() else { panic!() };
    assert_eq!(def.foreign_keys[0].on_delete, FkAction::Cascade);
    // A table pointing to itself is offered as a target.
    assert!(form.targets().contains(&TableRef::new("invoices")));
}

#[test]
fn a_column_changed_keeps_what_wasnt_touched_and_a_rename_alone_stays_one() {
    let mut form = Form::new(Kind::AlterColumn("status".into()), ctx(Some("orders")));
    assert!(form.changes().is_err(), "nothing changed yet");
    form.columns[0].name.set_text("state");
    let changes = form.changes().unwrap();
    let [SchemaChange::AlterColumn { column, to, .. }] = changes.as_slice() else { panic!() };
    assert_eq!(column, "status");
    // The default as the catalog wrote it: the domain sees only the name changed.
    assert_eq!(to.default, Some(DefaultValue::Expression("'new'".into())));
    let simple = Sqlite.simplified(&changes[0], &orders());
    assert!(matches!(simple, SchemaChange::RenameColumn { .. }));
    form.columns[0].name.set_text("total");
    assert!(form.changes().is_err(), "a name taken");
    // A source that only renames: a rename.
    let mut form = Form::new(Kind::AlterColumn("total".into()), csv());
    assert_eq!(form.texts(), vec![Text::Name(0)]);
    form.columns[0].name.set_text("amount");
    assert!(matches!(form.changes().unwrap().as_slice(), [SchemaChange::RenameColumn { .. }]));
}

#[test]
fn defaults_are_read_by_the_columns_type() {
    let value = |typed: &str, ty: &str| parse_default(typed, ty).unwrap();
    assert_eq!(value("", "TEXT"), None);
    assert_eq!(value("null", "INTEGER"), None);
    assert_eq!(value("42", "INTEGER"), Some(DefaultValue::Value(Value::Int(42))));
    assert_eq!(value("1.5", "REAL"), Some(DefaultValue::Value(Value::Float(1.5))));
    assert_eq!(value("true", "BOOLEAN"), Some(DefaultValue::Value(Value::Bool(true))));
    assert_eq!(value("'it''s'", "INTEGER"), Some(DefaultValue::Value(Value::Text("it's".into()))));
    assert_eq!(value("new", "TEXT"), Some(DefaultValue::Value(Value::Text("new".into()))));
    assert_eq!(value("current_timestamp", "TEXT"), Some(DefaultValue::CurrentTimestamp));
    assert_eq!(
        value("datetime('now')", "TEXT"),
        Some(DefaultValue::Expression("datetime('now')".into()))
    );
    assert!(parse_default("x", "INTEGER").is_err());
    assert!(parse_default("(1); DROP TABLE t", "TEXT").is_err());
}

#[test]
fn an_index_follows_its_columns_until_named_and_checks_its_condition() {
    let mut form = Form::new(Kind::CreateIndex, ctx(Some("orders")));
    assert!(form.changes().is_err(), "no columns");
    form.toggle_index_column("status");
    form.toggle_index_column("total");
    form.flip_index_order("total");
    form.set_unique(true);
    assert_eq!(form.index_name.text(), "orders_status_total_unique");
    form.predicate.set_text("status <> 'cancelled'");
    let changes = form.changes().unwrap();
    let [SchemaChange::CreateIndex { index, .. }] = changes.as_slice() else { panic!() };
    assert_eq!(index.columns, vec!["status".to_owned(), "total".to_owned()]);
    assert_eq!(index.descending, vec![false, true]);
    assert!(index.unique);
    assert_eq!(index.predicate.as_deref(), Some("status <> 'cancelled'"));
    form.predicate.set_text("1; DROP TABLE orders");
    assert!(form.changes().is_err());
    form.predicate.set_text("");
    form.index_name.set_text("orders_total");
    form.edited(Text::IndexName);
    assert!(form.changes().is_err(), "a name taken");
    form.toggle_index_column("total");
    assert_eq!(form.index_name.text(), "orders_total", "typed: kept");
}

#[test]
fn drops_moves_and_renames_of_tables() {
    let drop = Form::new(Kind::DropColumn("total".into()), ctx(Some("orders")));
    assert!(matches!(drop.changes().unwrap().as_slice(), [SchemaChange::DropColumn { .. }]));
    let mut moved = Form::new(Kind::MoveColumn("status".into()), ctx(Some("orders")));
    assert_eq!(moved.position, 2);
    assert!(moved.changes().is_err(), "where it is");
    moved.position = 0;
    assert!(matches!(
        moved.changes().unwrap().as_slice(),
        [SchemaChange::MoveColumn { to: 0, .. }]
    ));
    let mut renamed = Form::new(Kind::RenameTable, ctx(Some("orders")));
    assert!(renamed.changes().is_err(), "the same name");
    renamed.table_name.set_text("customers");
    assert!(renamed.changes().is_err(), "taken");
    renamed.table_name.set_text("sales");
    assert!(matches!(renamed.changes().unwrap().as_slice(), [SchemaChange::RenameTable { .. }]));
    let index = Form::new(Kind::DropIndex("orders_total".into()), ctx(Some("orders")));
    assert!(index.kind.drops() && index.changes().is_ok());
    let _ = DbError::Cancelled;
}
