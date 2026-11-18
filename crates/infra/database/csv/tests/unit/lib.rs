use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use database::{
    Collect, CompareOp, Filter, KeyKind, Order, Page, RowKey, Session, TableRef, Target, TypeClass,
    Value,
};

use super::*;

/// A folder of the system's temp dir, gone with it (never the user's files).
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "db-csv-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// `name` written with `bytes`.
    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// The names of what's in it.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn target(path: &Path) -> Target {
    Target { database: path.to_string_lossy().into_owned(), ..Target::default() }
}

pub(crate) fn open(path: &Path) -> CsvSession {
    CsvSession::open(&target(path)).unwrap()
}

/// Small chunks and marks: an index built in many steps.
pub(crate) const SMALL: Tuning = Tuning { every: 64, chunk: 4096, buffer: 1024 };

pub(crate) fn table(session: &CsvSession) -> TableRef {
    session.table.clone()
}

pub(crate) fn read(session: &mut CsvSession, page: &Page) -> Collect {
    let mut rows = Collect::default();
    let table = table(session);
    session.read(&table, page, &mut rows, &Cancel::default()).unwrap();
    rows
}

pub(crate) fn texts(rows: &Collect) -> Vec<Vec<String>> {
    rows.rows.iter().map(|r| r.iter().map(Value::display).collect()).collect()
}

fn page(offset: u64, limit: u32) -> Page {
    Page { offset, ..Page::first(limit) }
}

/// `n` rows: `id,name,score,note`, every 7th note two lines (quoted), every 5th empty.
pub(crate) fn generated(n: usize) -> Vec<u8> {
    let mut out = b"id,name,score,note\n".to_vec();
    for i in 0..n {
        let note = match i {
            i if i % 7 == 0 => format!("\"line one\nline \"\"{i}\"\"\""),
            i if i % 5 == 0 => String::new(),
            i => format!("note {i}"),
        };
        out.extend_from_slice(format!("{i},user {i},{},{note}\n", i % 100).as_bytes());
    }
    out
}

#[test]
fn the_delimiter_is_sniffed_or_known_by_the_extension() {
    let dir = TempDir::new("dialects");
    let semicolon = dir.file("prices.csv", b"name;price\napple;2,50\npear;3\n");
    let comma = dir.file("people.csv", b"name,age\nana,31\nbob,42\n");
    let tabbed = dir.file("tabbed.csv", b"a\tb\tc\n1\t2\t3\n");
    let tsv = dir.file("data.tsv", b"x,y\ty\n1,5\t2\n");
    let cases = [(&semicolon, b';'), (&comma, b','), (&tabbed, b'\t'), (&tsv, b'\t')];
    for (path, delimiter) in cases {
        assert_eq!(open(path).layout().dialect.delimiter, delimiter, "{}", path.display());
    }
    let prices = open(&semicolon);
    let columns: Vec<(&str, TypeClass)> =
        prices.layout().columns.iter().map(|c| (c.name.as_str(), c.class)).collect();
    assert_eq!(columns, [("name", TypeClass::Text), ("price", TypeClass::Decimal)]);
    assert_eq!(prices.table.name, "prices");
    // A tsv's comma is a character of its fields.
    let mut tsv = open(&tsv);
    assert_eq!(texts(&read(&mut tsv, &Page::first(10))), [["1,5", "2"]]);
    // The target says otherwise.
    let mut told = target(&comma);
    told.options.insert("delimiter".into(), ";".into());
    assert_eq!(CsvSession::open(&told).unwrap().layout().dialect.delimiter, b';');
}

#[test]
fn a_header_names_the_columns_or_they_are_numbered() {
    let dir = TempDir::new("header");
    let plain = dir.file("plain.csv", b"1,2024-01-02,x\n2,2024-01-03,y\n");
    let mut session = open(&plain);
    assert!(!session.layout().dialect.header);
    let names: Vec<&str> = session.layout().columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["column_1", "column_2", "column_3"]);
    assert_eq!(session.layout().columns[1].class, TypeClass::DateTime);
    assert_eq!(read(&mut session, &Page::first(10)).rows.len(), 2, "no row is a header");

    // Told there's one; a name repeated is numbered.
    let mut told = target(&plain);
    told.options.insert("header".into(), "true".into());
    let session = CsvSession::open(&told).unwrap();
    let names: Vec<&str> = session.layout().columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["1", "2024-01-02", "x"]);
    let twice = dir.file("twice.csv", b"name,name,\nana,bob,c\n");
    let mut told = target(&twice);
    told.options.insert("header".into(), "on".into());
    let session = CsvSession::open(&told).unwrap();
    let names: Vec<&str> = session.layout().columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["name", "name_2", "column_3"]);
}

#[test]
fn the_table_and_its_structure_find_rows_by_line() {
    let dir = TempDir::new("structure");
    let path = dir.file("people.csv", b"id,name\n1,ana\n2,bob\n");
    let mut session = open(&path);
    let tables = session.tables(&[]).unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!((tables[0].table.name.as_str(), tables[0].rows), ("people", Some(2)));
    let structure = session.structure(&TableRef::new("people")).unwrap();
    assert_eq!(structure.key_kind(), KeyKind::Line);
    assert_eq!(structure.columns[0].declared_type, "INTEGER");
    assert!(session.structure(&TableRef::new("other")).is_err());
    let nodes = session.children(&["people".to_owned()]).unwrap();
    assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["id", "name"]);
    assert!(matches!(
        session.execute("SELECT 1", &mut Collect::default(), &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
    let rows = read(&mut session, &Page::first(10));
    assert_eq!(rows.rows[1], vec![Value::Int(2), Value::Text("bob".into())]);
    assert_eq!(
        rows.keys[1],
        RowKey::Line { line: 1, fingerprint: database::fingerprint(&rows.rows[1]) }
    );
}

#[test]
fn quoted_line_breaks_stay_in_their_fields_across_pages() {
    let dir = TempDir::new("quoted");
    let path = dir.file(
        "notes.csv",
        b"id,note\r\n1,\"a\r\nb\"\r\n\r\n2,\"c,\"\"d\"\"\"\r\n3,\"\n\n\"\r\n4,e",
    );
    let mut session = CsvSession::open_tuned(&target(&path), Tuning { every: 2, ..SMALL }).unwrap();
    let all = texts(&read(&mut session, &Page::first(10)));
    assert_eq!(all, [["1", "a\r\nb"], ["2", "c,\"d\""], ["3", "\n\n"], ["4", "e"]]);
    for offset in 0..4 {
        let one = texts(&read(&mut session, &page(offset, 1)));
        assert_eq!(one, [all[offset as usize].clone()], "the page at {offset}");
    }
    assert!(read(&mut session, &page(4, 5)).rows.is_empty());
    let table = table(&session);
    assert_eq!(session.count(&table, None, &Cancel::default()), Ok(4));
}

#[test]
fn pages_of_a_large_file_are_read_by_seek_over_an_index_built_in_chunks() {
    let dir = TempDir::new("large");
    let path = dir.file("large.csv", &generated(50_000));
    let mut session = CsvSession::open_tuned(&target(&path), SMALL).unwrap();
    // Before the total: an estimate, the index only as far as the page.
    let first = read(&mut session, &page(100, 3));
    assert_eq!(texts(&first)[0][..3], ["100", "user 100", "0"]);
    assert!(!session.index().done());
    assert!(session.index().records() < 1_000, "{}", session.index().records());
    let estimate = session.tables(&[]).unwrap()[0].rows.unwrap();
    assert!((40_000..60_000).contains(&estimate), "{estimate}");

    for offset in [25_000u64, 49_997, 7, 0] {
        let rows = read(&mut session, &page(offset, 5));
        let ids: Vec<String> = texts(&rows).iter().map(|r| r[0].clone()).collect();
        let expected: Vec<String> =
            (offset..(offset + 5).min(50_000)).map(|i| i.to_string()).collect();
        assert_eq!(ids, expected, "the page at {offset}");
        if offset == 49_997 {
            assert_eq!(rows.rows.len(), 3);
        }
    }
    let row_7 = read(&mut session, &page(7, 1));
    assert_eq!(texts(&row_7)[0][3], "line one\nline \"7\"");
    let table = table(&session);
    assert_eq!(session.count(&table, None, &Cancel::default()), Ok(50_000));
    assert!(session.index().done());
    // A mark every 64 records: memory of the order of the file's rows / 64.
    assert_eq!(session.index().marks(), 50_000usize.div_ceil(64));
    assert_eq!(session.tables(&[]).unwrap()[0].rows, Some(50_000));
}

#[test]
fn indexing_is_cancelled_between_chunks_and_resumed() {
    let dir = TempDir::new("cancel");
    let path = dir.file("large.csv", &generated(50_000));
    let mut session = CsvSession::open_tuned(&target(&path), SMALL).unwrap();
    let table = table(&session);
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(session.count(&table, None, &cancel), Err(DbError::Cancelled));
    // A chunk was scanned, kept; not the whole file.
    let found = session.index().records();
    assert!(found > 0 && found < 50_000, "{found}");
    assert!(!session.index().done());
    // A page within it, and the next request going on from there.
    assert_eq!(texts(&read(&mut session, &page(10, 1)))[0][0], "10");
    assert_eq!(session.count(&table, None, &Cancel::default()), Ok(50_000));
    // A sort cancelled the same way.
    let sorted =
        Page { order: vec![Order { column: "id".into(), descending: true }], ..Page::first(3) };
    let mut rows = Collect::default();
    assert_eq!(session.read(&table, &sorted, &mut rows, &cancel), Err(DbError::Cancelled));
}

#[test]
fn rows_are_sorted_and_filtered_in_the_file() {
    let dir = TempDir::new("sorted");
    let path = dir.file(
        "scores.csv",
        b"name,team,score\nana,red,10\nbob,blue,9\ncid,red,30\ndan,blue,\neve,red,9\n",
    );
    let mut session = open(&path);
    let table = table(&session);
    let by_score = Page {
        order: vec![
            Order { column: "score".into(), descending: true },
            Order { column: "name".into(), descending: false },
        ],
        ..Page::first(10)
    };
    let rows = read(&mut session, &by_score);
    let names: Vec<String> = texts(&rows).iter().map(|r| r[0].clone()).collect();
    assert_eq!(
        names,
        ["cid", "ana", "bob", "eve", "dan"],
        "numbers by value, NULL last descending"
    );
    // Each row keeps its own key (its line in the file).
    assert!(matches!(rows.keys[0], RowKey::Line { line: 2, .. }));

    let red = Filter::Compare {
        column: "team".into(),
        op: CompareOp::Eq,
        value: Value::Text("red".into()),
    };
    let filtered = Page { offset: 1, filter: Some(red.clone()), ..by_score.clone() };
    let names: Vec<String> =
        texts(&read(&mut session, &filtered)).iter().map(|r| r[0].clone()).collect();
    assert_eq!(names, ["ana", "eve"]);
    assert_eq!(session.count(&table, Some(&red), &Cancel::default()), Ok(3));
    assert_eq!(
        session.count(&table, Some(&Filter::IsNull("score".into())), &Cancel::default()),
        Ok(1)
    );
    let unknown =
        Page { order: vec![Order { column: "nope".into(), descending: false }], ..Page::first(1) };
    let mut rows = Collect::default();
    assert!(matches!(
        session.read(&table, &unknown, &mut rows, &Cancel::default()),
        Err(DbError::Query(_))
    ));
    assert_eq!(CAPABILITIES.sorting, Sorting::UpTo(SORT_LIMIT));
}

#[test]
fn a_file_changed_by_another_program_is_read_again() {
    let dir = TempDir::new("reload");
    let path = dir.file("people.csv", b"id,name\n1,ana\n");
    let mut session = open(&path);
    assert_eq!(read(&mut session, &Page::first(10)).rows.len(), 1);
    std::fs::write(&path, b"id;name;age\n1;ana;31\n2;bob;42\n").unwrap();
    let rows = read(&mut session, &Page::first(10));
    assert_eq!(texts(&rows), [["1", "ana", "31"], ["2", "bob", "42"]]);
    assert_eq!(session.layout().dialect.delimiter, b';');
}

#[test]
fn a_missing_file_or_a_binary_one_is_refused() {
    let dir = TempDir::new("missing");
    assert!(matches!(CsvSession::open(&target(&dir.0.join("nope.csv"))), Err(DbError::Connect(_))));
    assert!(matches!(CsvSession::open(&target(&dir.0)), Err(DbError::Connect(_))));
    assert!(is_text(b"a,b\n1,2"));
    assert!(!is_text(b"a\0,\0b"));
}

#[test]
fn a_read_only_source_is_never_written() {
    let dir = TempDir::new("read-only");
    let bytes = b"id,name\n1,ana\n";
    let path = dir.file("people.csv", bytes);
    let mut read_only = target(&path);
    read_only.options.insert(Target::READ_ONLY.into(), "true".into());
    let mut session = CsvSession::open(&read_only).unwrap();
    let rows = read(&mut session, &Page::first(1));
    let set = ChangeSet {
        table: table(&session),
        changes: vec![database::Change::Delete { key: rows.keys[0].clone() }],
    };
    assert!(matches!(session.apply(&set), Err(DbError::ReadOnly(_))));
    let drop = SchemaChange::DropColumn { table: table(&session), column: "name".into() };
    assert!(matches!(session.alter(&[drop]), Err(DbError::ReadOnly(_))));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(dir.names(), ["people.csv"]);
}

#[test]
fn a_larger_sample_reads_more_of_the_file_up_to_a_limit() {
    assert_eq!(sample_bytes(1), SAMPLE_BYTES);
    assert_eq!(sample_bytes(SAMPLE_ROWS), SAMPLE_BYTES);
    assert_eq!(sample_bytes(SAMPLE_ROWS * 4), SAMPLE_BYTES * 4);
    assert_eq!(sample_bytes(usize::MAX / 2), MAX_SAMPLE_BYTES);
}
