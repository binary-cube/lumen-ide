use super::*;

#[test]
fn a_files_name_tells_its_dialect() {
    let of = |name: &str| Dialect::of_path(Path::new(name));
    assert_eq!(of("schema.sql"), Dialect::Standard);
    assert_eq!(of("dir/report.pgsql"), Dialect::Postgres);
    assert_eq!(of("report.PSQL"), Dialect::Postgres);
    assert_eq!(of("x.pg.sql"), Dialect::Postgres);
    assert_eq!(of("x.postgres.sql"), Dialect::Postgres);
    assert_eq!(of("app.sqlite.sql"), Dialect::Sqlite);
    assert_eq!(of("app.SQLite3.sql"), Dialect::Sqlite);
    assert_eq!(of("dump.mysql.sql"), Dialect::MySql);
    assert_eq!(of("dump.mariadb.sql"), Dialect::MySql);
    assert_eq!(of("my.notes.sql"), Dialect::Standard);
    assert_eq!(of("schema.ddl"), Dialect::Standard);
    assert_eq!(of(""), Dialect::Standard);
}

#[test]
fn the_lists_are_capitals_sorted_once() {
    for list in [STANDARD, STANDARD_FUNCTIONS, VALUES]
        .into_iter()
        .chain(Dialect::ALL.iter().map(|d| d.own_keywords()))
    {
        assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
        assert!(list.iter().all(|w| w.chars().all(|c| c.is_ascii_uppercase() || c == '_')));
    }
    // A dialect's own words aren't the standard's again.
    for dialect in Dialect::ALL {
        for word in dialect.own_keywords() {
            assert!(!STANDARD.contains(word), "{dialect:?} {word}");
        }
    }
    assert!(VALUES.iter().all(|v| STANDARD.contains(v)));
}

#[test]
fn each_dialect_has_its_own_words() {
    assert!(Dialect::Sqlite.own_keywords().contains(&"PRAGMA"));
    assert!(Dialect::Postgres.own_keywords().contains(&"ILIKE"));
    assert!(Dialect::MySql.own_keywords().contains(&"AUTO_INCREMENT"));
    assert!(!Dialect::Postgres.own_keywords().contains(&"PRAGMA"));
    assert!(!Dialect::Sqlite.own_keywords().contains(&"ILIKE"));
    for d in Dialect::ALL {
        assert_eq!(Dialect::ALL[d.index()], d);
    }
}
