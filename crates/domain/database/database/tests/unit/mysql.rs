use super::*;
use crate::{
    Change, ChangeSet, ColumnInfo, Filter, ForeignKey, Generated, KeyKind, Page, SchemaChange,
    Structure, TableRef, Value,
};

fn t(schema: &str, name: &str) -> TableRef {
    TableRef { schema: Some(schema.to_owned()), name: name.to_owned() }
}

#[test]
fn versions_are_read_from_what_the_server_says() {
    assert_eq!(ServerVersion::parse("8.0.36"), Some(ServerVersion::mysql(80036)));
    assert_eq!(ServerVersion::parse("5.7.44-log"), Some(ServerVersion::mysql(50744)));
    assert_eq!(ServerVersion::parse("8.4.3"), Some(ServerVersion::mysql(80403)));
    assert_eq!(
        ServerVersion::parse("10.11.10-MariaDB-ubu2204"),
        Some(ServerVersion::mariadb(101110))
    );
    assert_eq!(ServerVersion::parse("5.5.5-11.4.4-MariaDB"), Some(ServerVersion::mariadb(110404)));
    assert_eq!(ServerVersion::parse("11.8"), Some(ServerVersion::mysql(110800)));
    assert_eq!(ServerVersion::parse("x"), None);
    assert_eq!(ServerVersion::mariadb(101106).text(), "10.11.6");
    assert_eq!(ServerVersion::mariadb(101106).engine(), "MariaDB");
}

#[test]
fn features_by_engine_and_version() {
    let my57 = ServerVersion::mysql(50744);
    let my80 = ServerVersion::mysql(80036);
    let maria = ServerVersion::mariadb(101106);
    assert!(!my57.has(Feature::RenameColumn) && my80.has(Feature::RenameColumn));
    assert!(
        maria.has(Feature::RenameColumn)
            && !ServerVersion::mariadb(100410).has(Feature::RenameColumn)
    );
    assert!(
        my80.has(Feature::ExplainAnalyze)
            && !ServerVersion::mysql(80017).has(Feature::ExplainAnalyze)
    );
    assert!(!maria.has(Feature::ExplainAnalyze) && maria.has(Feature::AnalyzeJson));
    assert!(maria.has(Feature::Returning) && !my80.has(Feature::Returning));
    assert!(!maria.has(Feature::FunctionalIndex) && my80.has(Feature::FunctionalIndex));
    assert!(is_system_schema("Performance_Schema") && !is_system_schema("shop"));
}

#[test]
fn names_and_literals_read_the_same_whatever_the_sql_mode() {
    let d = MySql::default();
    assert_eq!(d.quote_identifier("we`ird").unwrap(), "`we``ird`");
    assert_eq!(d.table_name(&t("shop", "orders")).unwrap(), "`shop`.`orders`");
    assert_eq!(d.literal(&Value::Text("it's".into())).unwrap(), "'it''s'");
    // A backslash would be an escape (or not, by `sql_mode`): hex instead.
    assert_eq!(d.literal(&Value::Text(r"a\b".into())).unwrap(), "_utf8mb4 X'615C62'");
    assert_eq!(d.literal(&Value::Bytes(vec![0xde, 0xad])).unwrap(), "X'DEAD'");
    assert_eq!(d.literal(&Value::Bool(true)).unwrap(), "TRUE");
    assert_eq!(d.literal(&Value::Json("{\"a\":1}".into())).unwrap(), "'{\"a\":1}'");
    assert!(d.literal(&Value::Float(f64::NAN)).is_err());
    assert_eq!(d.flavor(), Flavor::MySql);
    assert_eq!(d.name(), "mysql");
}

#[test]
fn a_filter_casts_as_char_and_escapes_without_backslashes() {
    let d = MySql::default();
    let sql = d.filter(&Filter::Contains { column: "name".into(), text: "50%_a!".into() }).unwrap();
    assert_eq!(sql, "LOWER(CAST(`name` AS CHAR)) LIKE '%50!%!_a!!%' ESCAPE '!'");
    let page = Page { filter: Some(Filter::IsNull("note".into())), ..Page::first(10) };
    assert_eq!(
        d.select(&t("shop", "orders"), &[], &page).unwrap(),
        "SELECT * FROM `shop`.`orders` WHERE `note` IS NULL LIMIT 10"
    );
}

#[test]
fn columns_are_defined_in_the_order_every_version_takes() {
    let mut id = ColumnDef::new("id", "bigint unsigned");
    id.auto_increment = true;
    id.nullable = false;
    assert_eq!(
        MySql::default().column_definition(&id).unwrap(),
        "`id` bigint unsigned NOT NULL AUTO_INCREMENT"
    );
    let size = ColumnDef::new("size", "enum('S', 'M')")
        .not_null()
        .default(DefaultValue::Value(Value::Text("M".into())));
    assert_eq!(
        MySql::default().column_definition(&size).unwrap(),
        "`size` enum('S', 'M') NOT NULL DEFAULT 'M'"
    );
    let mut amount = ColumnDef::new("amount", "decimal(12, 2)");
    amount.generated = Some(Generated { expression: "quantity * price".into(), stored: true });
    assert_eq!(
        MySql::default().column_definition(&amount).unwrap(),
        "`amount` decimal(12, 2) GENERATED ALWAYS AS (quantity * price) STORED"
    );
    assert!(MySql::default().type_name("int); DROP TABLE x").is_err());
}

#[test]
fn defaults_by_version() {
    let old = MySql::new(ServerVersion::mysql(50744));
    let new = MySql::new(ServerVersion::mysql(80036));
    let expr = DefaultValue::Expression("uuid()".into());
    assert!(old.default_value(&expr).is_err());
    assert_eq!(new.default_value(&expr).unwrap(), "(uuid())");
    // A literal or the time: as is everywhere.
    assert_eq!(old.default_value(&DefaultValue::Expression("'M'".into())).unwrap(), "'M'");
    assert_eq!(old.default_value(&DefaultValue::Expression("0".into())).unwrap(), "0");
    assert_eq!(
        old.default_value(&DefaultValue::Expression("CURRENT_TIMESTAMP(3)".into())).unwrap(),
        "CURRENT_TIMESTAMP(3)"
    );
    assert_eq!(old.default_value(&DefaultValue::CurrentTimestamp).unwrap(), "CURRENT_TIMESTAMP");
    assert!(old.default_value(&DefaultValue::CurrentDate).is_err());
    assert_eq!(new.default_value(&DefaultValue::CurrentDate).unwrap(), "(CURRENT_DATE)");
}

#[test]
fn schema_changes_are_written_as_mysql_takes_them() {
    let d = MySql::new(ServerVersion::mysql(80036));
    let orders = t("shop", "orders");
    assert_eq!(
        d.schema(&SchemaChange::RenameTable { table: orders.clone(), to: "sales".into() }).unwrap(),
        ["ALTER TABLE `shop`.`orders` RENAME TO `shop`.`sales`"]
    );
    assert_eq!(
        d.schema(&SchemaChange::DropIndex { table: orders.clone(), name: "o_c".into() }).unwrap(),
        ["DROP INDEX `o_c` ON `shop`.`orders`"]
    );
    let to = ColumnDef::new("remark", "varchar(100)").not_null();
    assert_eq!(
        d.schema(&SchemaChange::AlterColumn { table: orders.clone(), column: "note".into(), to })
            .unwrap(),
        ["ALTER TABLE `shop`.`orders` CHANGE COLUMN `note` `remark` varchar(100) NOT NULL"]
    );
    let key = ForeignKey {
        name: Some("orders_customer_fk".into()),
        columns: vec!["customer_id".into()],
        table: t("shop", "customers"),
        references: vec!["id".into()],
        on_update: crate::FkAction::NoAction,
        on_delete: crate::FkAction::Cascade,
    };
    assert_eq!(
        d.schema(&SchemaChange::DropForeignKey { table: orders.clone(), key }).unwrap(),
        ["ALTER TABLE `shop`.`orders` DROP FOREIGN KEY `orders_customer_fk`"]
    );
    assert_eq!(
        d.schema(&SchemaChange::RenameColumn {
            table: orders.clone(),
            from: "note".into(),
            to: "remark".into()
        })
        .unwrap(),
        ["ALTER TABLE `shop`.`orders` RENAME COLUMN `note` TO `remark`"]
    );
    let partial = crate::IndexInfo {
        name: "p".into(),
        columns: vec!["note".into()],
        unique: false,
        implicit: false,
        descending: Vec::new(),
        predicate: Some("note IS NOT NULL".into()),
    };
    assert!(d.schema(&SchemaChange::CreateIndex { table: orders, index: partial }).is_err());
}

#[test]
fn a_rename_on_mysql_57_changes_the_column_with_its_definition() {
    let d = MySql::new(ServerVersion::mysql(50744));
    let orders = t("shop", "orders");
    let rename = SchemaChange::RenameColumn {
        table: orders.clone(),
        from: "note".into(),
        to: "remark".into(),
    };
    assert!(d.schema(&rename).is_err(), "not without the definition");
    let mut note = ColumnInfo::new("note", "text");
    note.nullable = true;
    let structure = Structure {
        table: orders,
        columns: vec![note],
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        rowid: false,
        lines: false,
    };
    let simple = d.simplified(&rename, &structure);
    assert_eq!(
        d.schema(&simple).unwrap(),
        ["ALTER TABLE `shop`.`orders` CHANGE COLUMN `note` `remark` text"]
    );
    // A newer server renames it as asked.
    let new = MySql::new(ServerVersion::mysql(80036));
    assert_eq!(new.simplified(&rename, &structure), rename);
}

#[test]
fn inserts_return_their_key_only_on_mariadb_105() {
    let set = ChangeSet {
        table: t("shop", "countries"),
        changes: vec![Change::Insert { values: vec![("code".into(), Value::Text("IT".into()))] }],
    };
    let key = KeyKind::Primary(vec!["code".into()]);
    assert_eq!(
        MySql::new(ServerVersion::mariadb(101106)).returning(&key, None).unwrap().as_deref(),
        Some(" RETURNING `code`")
    );
    assert_eq!(MySql::new(ServerVersion::mysql(80036)).returning(&key, None).unwrap(), None);
    assert_eq!(
        MySql::default().changes(&set).unwrap(),
        ["INSERT INTO `shop`.`countries` (`code`) VALUES ('IT')"]
    );
    assert_eq!(MySql::default().begin(), "START TRANSACTION");
}
