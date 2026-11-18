use super::*;
use crate::sql::lex;

const PG: Flavor = Flavor::Postgres { version: None };

fn kind(text: &str, flavor: Flavor) -> StatementKind {
    classify(text, &lex(text, flavor), flavor)
}

use DdlKind as D;
use StatementKind as K;
use TxControl as T;
use WriteKind as W;

#[test]
fn queries() {
    for text in [
        "SELECT 1",
        "select * from t where a = 'DELETE'",
        "(SELECT 1) UNION (SELECT 2)",
        "VALUES (1), (2)",
        "TABLE users",
        "SHOW search_path",
        "WITH a AS (SELECT 1) SELECT * FROM a",
        "WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r) SELECT n FROM r",
        "with a as materialized (select 1) select * from a",
        "SELECT * FROM t FOR UPDATE",
        "FETCH 10 FROM c",
        "-- leading comment\n/* and */ select 1",
    ] {
        assert_eq!(kind(text, PG), K::Query, "{text}");
        assert!(!kind(text, PG).writes());
    }
    assert_eq!(kind("DESCRIBE t", Flavor::MySql), K::Query);
    assert_eq!(kind("select * into @v from t", Flavor::MySql), K::Query, "MySQL: into variables");
}

#[test]
fn writes() {
    let cases: [(&str, StatementKind); 12] = [
        ("INSERT INTO t VALUES (1)", K::Write(W::Insert)),
        ("insert into t select * from u", K::Write(W::Insert)),
        ("UPDATE t SET a = 1", K::Write(W::Update)),
        ("DELETE FROM t", K::Write(W::Delete)),
        ("MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE", K::Write(W::Merge)),
        ("INSERT INTO t VALUES (1) ON CONFLICT (id) DO UPDATE SET a = 2", K::Write(W::Upsert)),
        ("INSERT INTO t VALUES (1) ON CONFLICT DO NOTHING", K::Write(W::Insert)),
        ("INSERT OR REPLACE INTO t VALUES (1)", K::Write(W::Upsert)),
        ("REPLACE INTO t VALUES (1)", K::Write(W::Upsert)),
        ("INSERT INTO t VALUES (1) ON DUPLICATE KEY UPDATE a = 2", K::Write(W::Upsert)),
        ("WITH a AS (SELECT 1) INSERT INTO t SELECT * FROM a", K::Write(W::Insert)),
        ("WITH d AS (DELETE FROM t RETURNING *) SELECT count(*) FROM d", K::Write(W::Delete)),
    ];
    for (text, expected) in cases {
        assert_eq!(kind(text, PG), expected, "{text}");
        assert!(expected.writes() && !expected.touches_schema() && !expected.is_query());
    }
    assert_eq!(
        kind("with a as (select 1), u as (update t set x = 1 returning *) select 1", PG),
        K::Write(W::Update),
        "a later part writing"
    );
    assert_eq!(
        kind("with u as (select 1 for update) select 1", PG),
        K::Query,
        "FOR UPDATE inside a part locks, it doesn't write"
    );
}

#[test]
fn schema_changes() {
    let cases: [(&str, StatementKind); 11] = [
        ("CREATE TABLE t (a int)", K::Ddl(D::Create)),
        ("create temp table t as select 1", K::Ddl(D::Create)),
        ("ALTER TABLE t ADD COLUMN b int", K::Ddl(D::Alter)),
        ("DROP TABLE t", K::Ddl(D::Drop)),
        ("TRUNCATE t", K::Ddl(D::Truncate)),
        ("RENAME TABLE a TO b", K::Ddl(D::Rename)),
        ("COMMENT ON TABLE t IS 'x'", K::Ddl(D::Comment)),
        ("GRANT SELECT ON t TO r", K::Ddl(D::Grant)),
        ("REVOKE ALL ON t FROM r", K::Ddl(D::Grant)),
        ("SELECT * INTO new_t FROM t", K::Ddl(D::Create)),
        ("IMPORT FOREIGN SCHEMA s FROM SERVER x INTO y", K::Ddl(D::Create)),
    ];
    for (text, expected) in cases {
        assert_eq!(kind(text, PG), expected, "{text}");
        assert!(expected.touches_schema() && expected.writes());
    }
    assert_eq!(
        kind("create trigger t after insert on x begin delete from y; end", Flavor::Sqlite),
        K::Ddl(D::Create)
    );
}

#[test]
fn transactions() {
    let cases: [(&str, TxControl); 12] = [
        ("BEGIN", T::Begin),
        ("BEGIN IMMEDIATE", T::Begin),
        ("begin transaction isolation level serializable", T::Begin),
        ("START TRANSACTION READ ONLY", T::Begin),
        ("COMMIT", T::Commit),
        ("END", T::Commit),
        ("END TRANSACTION", T::Commit),
        ("ROLLBACK", T::Rollback),
        ("ABORT", T::Rollback),
        ("ROLLBACK TO SAVEPOINT a", T::RollbackTo),
        ("SAVEPOINT a", T::Savepoint),
        ("RELEASE SAVEPOINT a", T::Release),
    ];
    for (text, expected) in cases {
        assert_eq!(kind(text, PG), K::Transaction(expected), "{text}");
        assert!(!K::Transaction(expected).writes());
    }
    assert_eq!(kind("rollback to a", Flavor::Sqlite), K::Transaction(T::RollbackTo));
}

#[test]
fn explain_runs_the_statement_only_with_analyze() {
    let cases: [(&str, StatementKind); 9] = [
        ("EXPLAIN SELECT 1", K::Explain { analyze: false, writes: false }),
        ("EXPLAIN QUERY PLAN SELECT * FROM t", K::Explain { analyze: false, writes: false }),
        ("EXPLAIN DELETE FROM t", K::Explain { analyze: false, writes: true }),
        ("EXPLAIN ANALYZE DELETE FROM t", K::Explain { analyze: true, writes: true }),
        ("EXPLAIN ANALYZE VERBOSE SELECT 1", K::Explain { analyze: true, writes: false }),
        (
            "EXPLAIN (ANALYZE, BUFFERS) UPDATE t SET a = 1",
            K::Explain { analyze: true, writes: true },
        ),
        (
            "explain (analyze true, format json) insert into t values (1)",
            K::Explain { analyze: true, writes: true },
        ),
        ("EXPLAIN (ANALYZE false) DELETE FROM t", K::Explain { analyze: false, writes: true }),
        ("EXPLAIN (FORMAT JSON) (SELECT 1)", K::Explain { analyze: false, writes: false }),
    ];
    for (text, expected) in cases {
        assert_eq!(kind(text, PG), expected, "{text}");
    }
    assert!(K::Explain { analyze: true, writes: true }.writes());
    assert!(!K::Explain { analyze: false, writes: true }.writes());
    assert!(!K::Explain { analyze: true, writes: false }.writes());
}

#[test]
fn session_and_pragmas() {
    for text in [
        "SET search_path TO x",
        "RESET ALL",
        "DISCARD ALL",
        "LISTEN ch",
        "DECLARE c CURSOR FOR SELECT 1",
    ] {
        assert_eq!(kind(text, PG), K::Session, "{text}");
    }
    assert_eq!(kind("USE shop", Flavor::MySql), K::Session);
    let sqlite = |text| kind(text, Flavor::Sqlite);
    assert_eq!(sqlite("ATTACH 'x.db' AS x"), K::Session);
    assert_eq!(sqlite("DETACH x"), K::Session);
    assert_eq!(sqlite("PRAGMA foreign_keys = ON"), K::Session);
    assert_eq!(sqlite("PRAGMA main.journal_mode = WAL"), K::Session);
    assert_eq!(sqlite("PRAGMA cache_size(100)"), K::Session);
    assert_eq!(sqlite("PRAGMA optimize"), K::Session);
    assert_eq!(sqlite("PRAGMA foreign_keys"), K::Query);
    assert_eq!(sqlite("PRAGMA table_info(users)"), K::Query);
    assert_eq!(sqlite("pragma main.index_list('t')"), K::Query);
    assert_eq!(sqlite("PRAGMA integrity_check"), K::Query);
}

#[test]
fn what_isnt_known_counts_as_a_write() {
    for text in [
        "CALL p()",
        "DO $$ BEGIN END $$",
        "COPY t FROM stdin",
        "VACUUM",
        "LOCK TABLE t",
        "",
        "(",
        "42",
    ] {
        let k = kind(text, PG);
        assert_eq!(k, K::Other, "{text:?}");
        assert!(k.writes());
    }
    assert_eq!(kind("WITH a AS (", PG), K::Query, "unfinished: nothing written yet");
}
