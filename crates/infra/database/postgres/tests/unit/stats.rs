use super::*;

const VERSIONS: [u32; 14] = [
    90_200, 90_300, 90_400, 90_500, 90_600, 100_000, 110_000, 120_000, 130_000, 140_000, 150_000,
    160_000, 170_000, 180_000,
];

fn row(values: &[Option<&str>]) -> Vec<Option<String>> {
    values.iter().map(|v| v.map(str::to_owned)).collect()
}

#[test]
fn the_tables_query_takes_what_each_version_has() {
    for v in VERSIONS {
        let sql = tables_sql(v, 2);
        assert!(sql.contains("($1::text, $2::text), ($3::text, $4::text)"), "{v}");
        assert!(!sql.contains("$5"), "{v}");
        assert!(sql.contains("pg_stat_user_tables") && sql.contains("pg_total_relation_size"));
        assert!(sql.contains("pg_table_is_visible"));
        assert_eq!(sql.contains("n_mod_since_analyze"), v >= 90_400, "{v}");
        assert_eq!(sql.contains("n_ins_since_vacuum"), v >= 130_000, "{v}");
        assert_eq!(sql.contains("last_seq_scan"), v >= 160_000, "{v}");
        assert_eq!(sql.contains("last_idx_scan"), v >= 160_000, "{v}");
        // Always 23 columns, in table_row's order.
        let select = &sql[sql.find("SELECT").unwrap()..sql.find(" FROM wanted").unwrap()];
        assert_eq!(select.matches("::text").count(), 23, "{v}: {select}");
    }
    assert!(tables_sql(170_000, 0).contains("($1::text, $2::text)"), "never an empty VALUES");
}

#[test]
fn the_indexes_query_reads_the_key_columns_of_the_version() {
    for v in VERSIONS {
        let sql = indexes_sql(v, 1);
        assert_eq!(sql.contains("indnkeyatts"), v >= 110_000, "{v}");
        assert_eq!(sql.contains("indnatts"), v < 110_000, "{v}");
        assert!(sql.contains("pg_stat_user_indexes") && sql.contains("chr(31)"));
    }
}

#[test]
fn the_history_is_matched_only_from_14_by_its_columns() {
    for v in VERSIONS {
        assert_eq!(history_sql(v, "public").is_some(), v >= 140_000, "{v}");
        let sql = history_query(v, "public");
        assert!(sql.contains("queryid = $1::text::bigint"));
        assert!(sql.contains("FROM \"public\".pg_stat_statements"));
        assert_eq!(sql.contains("total_exec_time"), v >= 130_000, "{v}");
        assert_eq!(sql.contains("sum(total_time)"), v < 130_000, "{v}");
        assert_eq!(sql.contains("wal_bytes"), v >= 130_000, "{v}");
        assert_eq!(sql.contains("min_"), v >= 90_500, "{v}");
    }
    assert!(history_query(170_000, "we\"ird").contains("\"we\"\"ird\".pg_stat_statements"));
}

#[test]
fn the_parameters_name_each_table() {
    let tables = [TableRef::in_schema("public", "orders"), TableRef::new("customers")];
    assert_eq!(params(&tables), ["public", "orders", "", "customers"]);
}

#[test]
fn a_tables_row_becomes_its_statistics() {
    let r = row(&[
        Some("public"),
        Some("orders"),
        Some("1.2e+06"),
        Some("15400"),
        Some("1200000"),
        Some("84000"),
        Some("412"),
        Some("90321"),
        None,
        Some("2026-10-29 03:12:44+00"),
        Some("2026-10-20 01:00:00+00"),
        Some("2026-10-28 02:00:00+00"),
        Some("3100"),
        Some("126156800"),
        Some("54525952"),
        Some("180682752"),
        Some("1"),
        Some("14"),
        Some("0"),
        Some("22"),
        Some("900"),
        Some("2026-10-30 11:59:00+00"),
        None,
    ]);
    let s = table_row(&r);
    assert_eq!(s.table, TableRef::in_schema("public", "orders"));
    assert_eq!((s.live_rows, s.dead_rows), (Some(1_200_000), Some(84_000)));
    assert_eq!((s.seq_scans, s.idx_scans), (Some(412), Some(90_321)));
    assert_eq!(s.last_analyze, None);
    assert_eq!(s.last_autoanalyze.as_deref(), Some("2026-10-29 03:12:44+00"));
    assert_eq!(s.last_vacuum.as_deref(), Some("2026-10-28 02:00:00+00"), "the later one");
    assert_eq!(s.modified_since_analyze, Some(3100));
    assert_eq!(
        (s.table_bytes, s.index_bytes, s.total_bytes),
        (Some(126_156_800), Some(54_525_952), Some(180_682_752))
    );
    assert_eq!(s.stats_present, Some(true));
    let extra = |k: &str| database::profile::extra::find(&s.extra, k).map(|e| e.value.clone());
    assert_eq!(
        extra("reltuples"),
        Some(ExtraValue::Quantity(Quantity::new(1_200_000.0, Unit::Rows)))
    );
    assert_eq!(extra("autoanalyze_count").and_then(|v| v.number()), Some(22.0));
    assert_eq!(extra("n_ins_since_vacuum").and_then(|v| v.number()), Some(900.0));
    assert_eq!(extra("last_vacuum"), Some(ExtraValue::Text("2026-10-20 01:00:00+00".into())));
    assert!(extra("last_seq_scan").is_some() && extra("last_idx_scan").is_none());
}

#[test]
fn a_table_never_analyzed_nor_counted_says_so() {
    let mut r = row(&[Some("public"), Some("fresh"), Some("-1"), Some("0")]);
    r.resize(23, None);
    let s = table_row(&r);
    assert_eq!(s.stats_present, None);
    assert_eq!((s.live_rows, s.last_vacuum.as_ref()), (None, None));
    assert!(database::profile::extra::find(&s.extra, "reltuples").is_none(), "-1: unknown");
}

#[test]
fn indexes_go_to_their_tables() {
    let mut table = row(&[Some("public"), Some("orders")]);
    table.resize(23, None);
    let index = row(&[
        Some("public"),
        Some("orders"),
        Some("idx_orders_status_created"),
        Some("false"),
        Some("false"),
        Some("status\u{1f}created_at"),
        Some("8192"),
        Some("0"),
        Some("10"),
        Some("4"),
        Some("(status = 'pending'::text)"),
        Some("false"),
    ]);
    let key = row(&[
        Some("public"),
        Some("orders"),
        Some("orders_pkey"),
        Some("true"),
        Some("true"),
        Some("id"),
        Some("16384"),
        Some("900"),
        None,
        None,
        None,
        Some("true"),
    ]);
    let other = row(&[Some("public"), Some("gone"), Some("gone_pkey"), Some("true")]);
    let all = assemble(&[table], &[index, key, other]);
    assert_eq!(all.len(), 1);
    let s = &all[0];
    assert_eq!(s.indexes.len(), 2);
    assert_eq!(s.indexes[0].columns, ["status", "created_at"]);
    assert_eq!(
        (s.indexes[0].unique, s.indexes[0].bytes, s.indexes[0].scans),
        (false, Some(8192), Some(0))
    );
    assert_eq!(s.indexes[1].columns, ["id"]);
    assert!(s.indexes[1].unique);
    let group = |name: &str, key: &str| {
        s.extra
            .iter()
            .find(|e| e.key == key && e.group.as_deref() == Some(&format!("index {name}")))
            .map(|e| e.value.clone())
    };
    assert_eq!(
        group("idx_orders_status_created", "predicate"),
        Some(ExtraValue::Text("(status = 'pending'::text)".into()))
    );
    assert_eq!(group("idx_orders_status_created", "valid"), Some(ExtraValue::Bool(false)));
    assert_eq!(group("orders_pkey", "primary"), Some(ExtraValue::Bool(true)));
    assert_eq!(group("orders_pkey", "valid"), None, "a valid index says nothing");
}

#[test]
fn the_history_row_is_its_figures_or_nothing() {
    let r = row(&[
        Some("18400"),
        Some("0.032"),
        Some("0.011"),
        Some("4.2"),
        None,
        Some("18400"),
        Some("0.995"),
        Some("0"),
        Some("0"),
    ]);
    let h = history_row(&r).unwrap();
    assert_eq!(
        (h.calls, h.mean_ms, h.min_ms, h.max_ms),
        (Some(18_400), Some(0.032), Some(0.011), Some(4.2))
    );
    assert_eq!((h.stddev_ms, h.rows, h.hit_ratio), (None, Some(18_400), Some(0.995)));
    assert_eq!((h.temp_blocks, h.wal_bytes), (Some(0), Some(0)));
    // No rows matched: sum() of nothing.
    assert_eq!(history_row(&row(&[None, None, None])), None);
    assert_eq!(history_row(&row(&[Some("0")])), None);
}
