use super::*;
use crate::postgres::Postgres;
use crate::sqlite::Sqlite;

#[test]
fn an_index_name_is_its_table_and_columns_cut_to_63_bytes() {
    let t = TableRef::new("Orders");
    assert_eq!(
        index_name(&t, &IndexSpec::on(vec!["Customer ID".into(), "status".into()])),
        "idx_orders_customer_id_status"
    );
    let long: Vec<String> = (0..20).map(|i| format!("column_number_{i}")).collect();
    let name = index_name(&t, &IndexSpec::on(long));
    assert_eq!(name.len(), MAX_NAME_BYTES.min(name.len()));
    assert!(name.len() <= MAX_NAME_BYTES && !name.ends_with('_'));
    let expr = IndexSpec { expression: Some("lower(\"email\")".into()), ..IndexSpec::default() };
    assert_eq!(index_name(&TableRef::new("users"), &expr), "idx_users_lower_email");
    // Not ASCII: replaced, never cut inside a character.
    assert_eq!(
        index_name(&TableRef::new("comenzi"), &IndexSpec::on(vec!["dată".into()])),
        "idx_comenzi_dat"
    );
}

#[test]
fn create_index_through_each_dialect() {
    let pg = Postgres::new(170_000);
    let t = TableRef::in_schema("shop", "orders");
    let spec = IndexSpec {
        include: vec!["total".into()],
        where_clause: Some("status = 'paid'".into()),
        ..IndexSpec::on(vec!["customer_id".into()])
    };
    let s = create_index(&pg, &t, &spec, true).unwrap();
    assert_eq!(
        s.sql(),
        Some(
            "CREATE INDEX CONCURRENTLY \"idx_orders_customer_id\" ON \"shop\".\"orders\" (\"customer_id\") INCLUDE (\"total\") WHERE status = 'paid';"
        )
    );
    // SQLite: the schema on the index, no INCLUDE, never CONCURRENTLY.
    let s = create_index(&Sqlite, &TableRef::in_schema("main", "orders"), &spec, true).unwrap();
    assert_eq!(
        s.sql(),
        Some(
            "CREATE INDEX \"main\".\"idx_orders_customer_id\" ON \"orders\" (\"customer_id\") WHERE status = 'paid';"
        )
    );
    let Suggestion::CreateIndex { concurrently, include, .. } = s else { panic!() };
    assert!(!concurrently && include.is_empty());
    // A name with a quote is quoted; a condition that isn't one expression is refused.
    let s =
        create_index(&pg, &TableRef::new("a\"b"), &IndexSpec::on(vec!["c".into()]), false).unwrap();
    assert_eq!(s.sql(), Some("CREATE INDEX \"idx_a_b_c\" ON \"a\"\"b\" (\"c\");"));
    let bad = IndexSpec {
        where_clause: Some("1; DROP TABLE x".into()),
        ..IndexSpec::on(vec!["c".into()])
    };
    assert!(create_index(&pg, &TableRef::new("t"), &bad, false).is_none());
    assert!(create_index(&pg, &TableRef::new("t"), &IndexSpec::default(), false).is_none());
}

#[test]
fn analyze_vacuum_statistics_and_settings() {
    let pg = Postgres::new(160_000);
    let t = TableRef::in_schema("public", "orders");
    assert_eq!(analyze(&pg, &t).unwrap().sql(), Some("ANALYZE \"public\".\"orders\";"));
    assert_eq!(vacuum(&pg, &t).unwrap().sql(), Some("VACUUM (ANALYZE) \"public\".\"orders\";"));
    assert!(vacuum(&Sqlite, &t).is_none(), "SQLite's VACUUM is the whole file");
    assert_eq!(analyze(&Sqlite, &TableRef::new("t")).unwrap().sql(), Some("ANALYZE \"t\";"));
    assert!(statistics(&pg, Some(160_000), &t, &["a".into()]).is_none(), "one column");
    assert!(statistics(&Sqlite, None, &t, &["a".into(), "b".into()]).is_none());
    assert_eq!(setting("work_mem", "64MB").unwrap().sql(), Some("SET work_mem = '64MB';"));
    assert_eq!(setting("jit", "off").unwrap().sql(), Some("SET jit = off;"));
    assert_eq!(
        setting("max_parallel_workers", "8").unwrap().sql(),
        Some("SET max_parallel_workers = 8;")
    );
    assert!(setting("work_mem", "1'; DROP").is_none());
    assert_eq!(memory_value(2_760_704 * 2), "8MB");
    assert_eq!(memory_value(1), "4MB");
    assert_eq!(memory_value(1_500 << 20), "2GB");
    assert_eq!(Suggestion::Rewrite { hint: RewriteHint::Batch }.sql(), None);
    assert_eq!(RewriteHint::KeysetPagination.id(), "keyset_pagination");
}
