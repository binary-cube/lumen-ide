//! The tables' statistics: what's asked and how the rows become them, without a server.

use super::*;

fn row(values: &[Option<&str>]) -> Vec<Option<String>> {
    values.iter().map(|v| v.map(str::to_owned)).collect()
}

#[test]
fn tables_are_asked_by_schema_and_name_the_sessions_database_for_none() {
    let tables =
        [TableRef::in_schema("shop", "orders"), TableRef { schema: None, name: "o'x".into() }];
    let sql = tables_sql(&tables);
    assert!(
        sql.contains("(TABLE_SCHEMA, TABLE_NAME) IN (('shop', 'orders'), (DATABASE(), 'o''x'))"),
        "{sql}"
    );
    assert!(indexes_sql(&tables).contains("information_schema.STATISTICS"));
    assert!(io_sql(&tables).contains("performance_schema.table_io_waits_summary_by_index_usage"));
}

#[test]
fn rows_become_each_tables_statistics_as_asked() {
    let asked = [TableRef { schema: None, name: "customers".into() }];
    let tables = [row(&[
        Some("shop"),
        Some("customers"),
        Some("InnoDB"),
        Some("1987"),
        Some("475136"),
        Some("98304"),
        Some("0"),
        Some("2001"),
        None,
        Some("2024-01-01 00:00:00"),
        Some("utf8mb4_0900_ai_ci"),
        Some("Dynamic"),
        Some("237"),
    ])];
    let indexes = [
        row(&[
            Some("shop"),
            Some("customers"),
            Some("PRIMARY"),
            Some("0"),
            Some("id"),
            Some("1987"),
        ]),
        row(&[
            Some("shop"),
            Some("customers"),
            Some("customers_email_key"),
            Some("0"),
            Some("email"),
            Some("1987"),
        ]),
        row(&[
            Some("shop"),
            Some("customers"),
            Some("customers_created"),
            Some("1"),
            Some("created_at"),
            Some("1900"),
        ]),
        row(&[Some("shop"), Some("other"), Some("PRIMARY"), Some("0"), Some("id"), Some("1")]),
    ];
    let io = [
        row(&[Some("shop"), Some("customers"), Some("PRIMARY"), Some("4000"), Some("4000")]),
        row(&[Some("shop"), Some("customers"), None, Some("0"), Some("2000")]),
    ];
    let stats = assemble(&asked, &tables, &indexes, &io);
    assert_eq!(stats.len(), 1);
    let s = &stats[0];
    assert_eq!(s.table, asked[0], "as the profile named it");
    assert_eq!(
        (s.live_rows, s.table_bytes, s.index_bytes, s.total_bytes),
        (Some(1987), Some(475136), Some(98304), Some(573440))
    );
    assert_eq!(s.indexes.len(), 3, "its own only");
    assert!(s.indexes[0].unique && !s.indexes[2].unique);
    assert_eq!(s.indexes[1].columns, ["email"]);
    let extra = |key: &str| s.extra.iter().find(|e| e.key == key).map(|e| e.value.text());
    assert_eq!(extra("engine").as_deref(), Some("InnoDB"));
    assert!(extra("rows_read_without_index").is_some() && extra("rows_fetched.PRIMARY").is_some());
    assert!(extra("update_time").is_none(), "unknown, not said");
    // A table not found: nothing for it.
    assert!(assemble(&[TableRef::in_schema("shop", "nope")], &tables, &[], &[]).is_empty());
}
