//! The catalog's queries by version: only what the oldest supported server reads, plus a
//! newer one's own columns where it has them.

use super::*;

#[test]
fn tables_by_version() {
    let old = tables(90200, false);
    assert!(old.contains("IN ('r', 'v', 'f')") && !old.contains("relispartition"), "{old}");
    assert!(tables(90600, false).contains("'m'"), "materialized views from 9.3");
    let new = tables(170000, true);
    assert!(new.contains("'p'") && new.contains("NOT c.relispartition"), "{new}");
    assert!(new.contains("n.nspname = $1"));
    // The rows estimated cheaply: the planner's, else the statistics' live rows (every
    // version has the function).
    for sql in [&old, &new] {
        assert!(sql.contains("c.reltuples") && sql.contains("pg_stat_get_live_tuples(c.oid)"));
    }
}

#[test]
fn columns_by_version() {
    let old = columns(90600);
    assert!(!old.contains("attidentity") && !old.contains("attgenerated"), "{old}");
    assert!(columns(100000).contains("attidentity") && !columns(110000).contains("attgenerated"));
    assert!(columns(120000).contains("a.attgenerated"));
}

#[test]
fn index_columns_leave_included_ones_out_where_there_are() {
    assert!(index_columns(100000).contains("i.indnatts"));
    assert!(index_columns(110000).contains("i.indnkeyatts"));
}

#[test]
fn nothing_newer_than_the_oldest_server_reads() {
    for version in [90200, 90600, 100000, 170000] {
        for sql in [
            tables(version, true),
            columns(version),
            indexes(),
            index_columns(version),
            foreign_keys(),
        ] {
            for newer in ["LATERAL", "WITH ORDINALITY", "array_position", "to_regclass"] {
                assert!(!sql.contains(newer), "{newer} in {sql}");
            }
        }
    }
}

#[test]
fn catalog_texts_are_read() {
    assert_eq!(numbers("{1,3}"), vec![1, 3]);
    assert_eq!(numbers("0 3"), vec![0, 3]);
    assert_eq!(unquote("\"Order \"\"x\"\"\""), "Order \"x\"");
    assert_eq!(unquote("plain"), "plain");
    assert_eq!(action("c"), database::FkAction::Cascade);
    assert_eq!(action("a"), database::FkAction::NoAction);
}

#[test]
fn the_consoles_columns_come_in_one_query_by_version() {
    for version in [90200, 90600, 100000, 110000, 120000, 170000, 180000] {
        let sql = catalog_columns(version, false);
        assert!(sql.contains("FROM pg_catalog.pg_attribute a"), "{sql}");
        assert!(
            sql.contains("JOIN pg_catalog.pg_class c") && sql.contains("pg_catalog.pg_namespace")
        );
        assert!(!sql.contains("array_position") && !sql.contains("LATERAL"), "9.2 reads it: {sql}");
        assert!(sql.contains("col_description"));
        assert_eq!(sql.contains("attgenerated"), version >= 120000, "{version}");
        assert_eq!(sql.contains("indnkeyatts"), version >= 110000, "{version}");
        assert_eq!(sql.contains("'p'"), version >= 100000, "partitioned tables: {version}");
        assert_eq!(sql.contains("'m'"), version >= 90300);
        assert!(!sql.contains("$1"));
    }
    assert!(catalog_columns(170000, true).contains("n.nspname = $1"));
}

#[test]
fn every_foreign_key_comes_in_one_query_that_9_2_reads() {
    let sql = catalog_keys(false);
    assert!(sql.contains("FROM pg_catalog.pg_constraint k") && sql.contains("k.contype = 'f'"));
    assert!(
        !sql.contains("LATERAL") && !sql.contains("ORDINALITY") && !sql.contains("array_position")
    );
    assert!(sql.contains("generate_series(1, 32)") && sql.contains("k.conkey[s.i]"));
    assert!(sql.contains("ORDER BY 1, 2, 3, s.i"), "a key's columns in order");
    assert!(!sql.contains("$1"));
    assert!(catalog_keys(true).contains("n.nspname = $1"));
    assert!(sql.contains("pg\\_toast"), "the server's own schemas left out");
}

#[test]
fn routines_are_read_by_prokind_from_11_and_proisagg_before() {
    for version in [90200, 90600, 100000] {
        let sql = routines(version, false);
        assert!(sql.contains("p.proisagg") && sql.contains("p.proiswindow"), "{sql}");
        assert!(!sql.contains("prokind"));
    }
    for version in [110000, 120000, 170000, 180000] {
        let sql = routines(version, true);
        assert!(sql.contains("p.prokind") && !sql.contains("proisagg"), "{sql}");
        assert!(sql.contains("n.nspname = $1"));
    }
    let sql = routines(90200, false);
    assert!(sql.contains("pg_get_function_arguments") && sql.contains("pg_get_function_result"));
    assert!(databases().contains("NOT d.datistemplate"));
}

#[test]
fn the_relations_tables_doesn_t_list_come_in_one_query() {
    let old = relations(90600);
    assert!(old.contains("c.relkind = 'S'"), "sequences");
    assert!(!old.contains("relispartition"), "9.6 has no partitions");
    let new = relations(170000);
    assert!(new.contains("c.relispartition"), "partitions from 10");
    assert!(new.contains("pg\\_toast") && !new.contains("$1"));
}

#[test]
fn schemas_and_databases_hide_nothing_the_user_may_reach() {
    // Every schema, whoever owns it: never `information_schema`, which hides another
    // role's schemas (before 10, every schema the user doesn't own).
    let all = schemas();
    assert!(all.contains("pg_catalog.pg_namespace") && !all.contains("information_schema."));
    // The databases it may connect to, no other.
    assert!(databases().contains("has_database_privilege(d.oid, 'CONNECT')"));
    assert!(databases().contains("NOT d.datistemplate"));
}
