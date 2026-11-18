// The places lit in a line: often only one range.
#![allow(clippy::single_range_in_vec_init)]
use super::*;

fn source() -> DataSourceId {
    DataSourceId::from_u128(7)
}

fn entry(text: &str, at: i64) -> HistoryEntry {
    HistoryEntry::new(source(), text, Flavor::Sqlite, &ParamStyles::of(Flavor::Sqlite), at)
}

fn texts(history: &QueryHistory) -> Vec<&str> {
    history.newest_first().map(|e| e.text.as_str()).collect()
}

#[test]
fn an_entry_masks_its_secrets_and_keeps_only_its_parameters_names() {
    let e = HistoryEntry::new(
        source(),
        "  CREATE USER bob WITH PASSWORD 'hunter2';\n",
        Flavor::Postgres { version: Some(170_000) },
        &ParamStyles::of(Flavor::Postgres { version: Some(170_000) }),
        5,
    );
    assert!(!e.text.contains("hunter2"), "{}", e.text);
    assert!(e.text.starts_with("CREATE USER"), "trimmed: {:?}", e.text);
    assert_eq!(e.at_ms, 5);
    let e = entry("SELECT * FROM t WHERE id = :id AND a = ? AND b = :id", 1);
    assert_eq!(e.params, [":id", "?"]);
    assert_eq!(e.text, "SELECT * FROM t WHERE id = :id AND a = ? AND b = :id");
    // A parameter in a string is text.
    assert!(entry("SELECT ':nope'", 1).params.is_empty());
}

#[test]
fn the_same_statement_run_again_moves_up_keeping_its_identity() {
    let mut h = QueryHistory::default();
    let first = entry("SELECT 1;", 1);
    let id = first.id;
    h.record(first, 10);
    h.record(entry("SELECT 2", 2), 10);
    let mut again = entry("SELECT   1", 3);
    again.rows = Some(1);
    h.record(again, 10);
    assert_eq!(texts(&h), ["SELECT   1", "SELECT 2"]);
    let top = h.newest_first().next().unwrap();
    assert_eq!((top.id, top.at_ms, top.rows), (id, 3, Some(1)));
    // Not the same statement: another case is another text.
    h.record(entry("select 1", 4), 10);
    assert_eq!(h.len(), 3);
}

#[test]
fn at_most_the_limit_is_kept_the_oldest_leaving() {
    let mut h = QueryHistory::default();
    for i in 0..5 {
        h.record(entry(&format!("SELECT {i}"), i), 3);
    }
    assert_eq!(texts(&h), ["SELECT 4", "SELECT 3", "SELECT 2"]);
}

#[test]
fn runs_are_appended_and_the_file_compacted_only_clearly_past_the_limit() {
    let mut h = QueryHistory::default();
    let limit = 20;
    let mut appended = 0;
    let mut compacted = None;
    for i in 0..60 {
        // Half of them the same statement again (a duplicate block in the file).
        let text = if i % 2 == 0 { "SELECT 1".to_owned() } else { format!("SELECT {i}") };
        match h.record(entry(&text, i), limit) {
            Write::Append(e) => {
                assert_eq!(e.at_ms, i);
                appended += 1;
            }
            Write::Compact(all) => {
                compacted.get_or_insert(i);
                assert!(all.len() <= limit);
                assert_eq!(h.written(), all.len());
            }
        }
    }
    // 20 + 16 blocks before the first rewrite.
    assert_eq!(compacted, Some(36));
    assert!(appended > 50);
    assert!(!needs_compaction(1000, 1000));
    assert!(!needs_compaction(1250, 1000));
    assert!(needs_compaction(1251, 1000));
}

#[test]
fn a_file_s_blocks_read_back_its_later_duplicates_winning() {
    let mut a = entry("SELECT a", 1);
    a.rows = Some(1);
    let b = entry("SELECT b", 2);
    let mut a2 = a.clone();
    a2.at_ms = 3;
    a2.rows = Some(9);
    let c = entry("SELECT c", 4);
    let h = QueryHistory::from_blocks(vec![a.clone(), b, a2, c], 10);
    assert_eq!(texts(&h), ["SELECT c", "SELECT a", "SELECT b"]);
    assert_eq!(h.get(a.id).map(|e| (e.at_ms, e.rows)), Some((3, Some(9))));
    assert_eq!(h.written(), 4);
    let h = QueryHistory::from_blocks(h.entries().to_vec(), 2);
    assert_eq!(texts(&h), ["SELECT c", "SELECT a"]);
}

#[test]
fn an_entry_is_deleted_and_the_rest_written_again() {
    let mut h = QueryHistory::default();
    let a = entry("SELECT a", 1);
    let id = a.id;
    h.record(a, 10);
    h.record(entry("SELECT b", 2), 10);
    let rest = h.remove(id).expect("it was there");
    assert_eq!(rest.len(), 1);
    assert_eq!(h.written(), 1);
    assert!(h.remove(id).is_none());
    h.clear();
    assert!(h.is_empty());
}

#[test]
fn the_search_wants_every_word_in_any_case() {
    let mut h = QueryHistory::default();
    h.record(entry("SELECT * FROM orders WHERE total > 10", 1), 10);
    h.record(entry("SELECT * FROM customers", 2), 10);
    h.record(entry("DELETE FROM orders WHERE id = 3", 3), 10);
    let found: Vec<&str> = h.search("orders where").iter().map(|e| e.text.as_str()).collect();
    assert_eq!(found, ["DELETE FROM orders WHERE id = 3", "SELECT * FROM orders WHERE total > 10"]);
    assert_eq!(h.search("").len(), 3);
    assert!(h.search("nothing").is_empty());
    let s = Search::new("from ORD");
    assert_eq!(s.places("SELECT x FROM orders"), [9..13, 14..17]);
    assert_eq!(Search::new("o or").places("orders"), [0..2]);
}

#[test]
fn the_place_and_the_outcomes_ids() {
    let mut e = entry("SELECT 1", 1);
    assert_eq!(e.place(), None);
    e.schema = Some("public".into());
    assert_eq!(e.place().as_deref(), Some("public"));
    e.database = Some("shop".into());
    assert_eq!(e.place().as_deref(), Some("shop.public"));
    for o in [Outcome::Ok, Outcome::Failed, Outcome::Cancelled] {
        assert_eq!(Outcome::from_id(o.id()), Some(o));
    }
    assert_eq!(HistoryId::parse(&e.id.to_string()), Some(e.id));
}

#[test]
fn the_memory_store_keeps_blocks_as_a_file_would() {
    let store = MemoryHistory::default();
    let a = entry("SELECT a", 1);
    store.append(source(), &a).unwrap();
    store.append(source(), &a).unwrap();
    assert_eq!(store.load(source()).unwrap().entries.len(), 2);
    store.rewrite(source(), std::slice::from_ref(&a)).unwrap();
    assert_eq!(store.blocks(source()), [a]);
    assert!(store.blocks(DataSourceId::from_u128(8)).is_empty());
    assert!(NoHistory.load(source()).unwrap().entries.is_empty());
}

/// Comparing a statement with a key word by word says what comparing their keys says.
#[test]
fn has_key_agrees_with_the_keys() {
    let texts = [
        "SELECT 1",
        "SELECT  1 ;",
        "select 1",
        "SELECT 1;;",
        "SELECT 1; ;",
        "SELECT 1;\n;",
        "\n  SELECT\t1  \n",
        "",
        ";",
        "  ",
        "SELECT 'a  b'",
        "SELECT 'a b'",
    ];
    for a in texts {
        for b in texts {
            assert_eq!(has_key(a, &key_of(b)), key_of(a) == key_of(b), "{a:?} / {b:?}");
        }
    }
}
