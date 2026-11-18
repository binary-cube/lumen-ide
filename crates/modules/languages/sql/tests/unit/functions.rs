use super::*;

#[test]
fn the_data_file_is_read_whole() {
    let functions = parse(SQLITE_FUNCTIONS);
    assert!(functions.len() > 100, "{}", functions.len());
    assert!(functions.windows(2).all(|w| w[0].name < w[1].name));
    let count = functions.iter().find(|f| f.name == "COUNT").unwrap();
    // An aggregate usable as a window function too is listed as a window one.
    assert_eq!((count.kinds, count.arities), ("w", "0,1"));
    let substr = functions.iter().find(|f| f.name == "SUBSTR").unwrap();
    assert_eq!(substr.arities, "2,3");
    assert!(functions.iter().any(|f| f.name == "JSON_EXTRACT"));
    assert!(SQLITE_FUNCTIONS.lines().next().unwrap().contains("cargo xtask sql-data"));
}

#[test]
fn a_line_without_its_fields_is_skipped() {
    let text: &'static str = "# header\nABS\ts\t1\nBROKEN\n\tx\ty\nMAX\ta,s\t-1,1\n";
    let names: Vec<&str> = parse(text).iter().map(|f| f.name).collect();
    assert_eq!(names, ["ABS", "MAX"]);
}

#[test]
fn each_dialect_offers_its_keywords_and_functions() {
    let words = load();
    let has = |d: Dialect, w: &str| words.of(d).binary_search(&w).is_ok();
    for d in Dialect::ALL {
        assert!(words.of(d).windows(2).all(|w| w[0] < w[1]), "{d:?} sorted, once each");
        assert!(has(d, "SELECT") && has(d, "COUNT") && has(d, "COALESCE"), "{d:?}");
    }
    assert!(has(Dialect::Sqlite, "PRAGMA") && has(Dialect::Sqlite, "JSON_EXTRACT"));
    assert!(has(Dialect::Sqlite, "IFNULL"));
    assert!(!has(Dialect::Postgres, "PRAGMA") && !has(Dialect::Postgres, "IFNULL"));
    assert!(has(Dialect::Postgres, "ILIKE") && has(Dialect::Postgres, "RETURNING"));
    assert!(has(Dialect::MySql, "AUTO_INCREMENT"));
    assert!(!has(Dialect::Standard, "PRAGMA") && !has(Dialect::Standard, "ILIKE"));
}

#[test]
fn a_keyword_is_never_a_call() {
    let words = load();
    assert!(words.is_call("COUNT"));
    assert!(words.is_call("CAST"));
    assert!(words.is_call("IFNULL"));
    // Functions too in SQLite, but keywords first.
    assert!(!words.is_call("REPLACE"));
    assert!(!words.is_call("LIKE"));
    assert!(!words.is_call("SELECT"));
    assert!(words.weight() > 0);
}
