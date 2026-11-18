use std::fs::File;

use database::Cancel;

use super::*;
use crate::tests::TempDir;

#[test]
fn records_are_read_whole_whatever_the_buffer() {
    let dir = TempDir::new("reader");
    let bytes = b"a,\"b\nc\"\n\n\"x\"\"\ny\",z\r\nlast";
    let path = dir.file("r.csv", bytes);
    for buffer in [1, 2, 3, 7, 64] {
        let mut reader =
            RecordReader::at(File::open(&path).unwrap(), 0, b',', b'"', buffer).unwrap();
        let mut record = Vec::new();
        let mut all = Vec::new();
        while let Some(start) = reader.next_into(&mut record).unwrap() {
            all.push((start, String::from_utf8(record.clone()).unwrap()));
        }
        assert_eq!(
            all,
            [
                (0, "a,\"b\nc\"\n".to_owned()),
                (8, "\n".to_owned()),
                (9, "\"x\"\"\ny\",z\r\n".to_owned()),
                (20, "last".to_owned())
            ],
            "a buffer of {buffer}"
        );
        // Blank lines skipped, from any record's start.
        let mut reader =
            RecordReader::at(File::open(&path).unwrap(), 8, b',', b'"', buffer).unwrap();
        assert_eq!(reader.next_record(&mut record).unwrap(), Some(9));
    }
}

#[test]
fn the_index_marks_every_nth_record_and_hashes_what_it_scanned() {
    let dir = TempDir::new("index");
    let mut bytes = b"\xEF\xBB\xBFh\n".to_vec();
    for i in 0..10 {
        bytes.extend_from_slice(format!("\"{i}\n\"\n").as_bytes());
    }
    let path = dir.file("i.csv", &bytes);
    let stamp = Stamp::of(&path).unwrap();
    let tuning = Tuning { every: 3, chunk: 1, buffer: 4 };
    let mut index = OffsetIndex::new(stamp, tuning, b',', b'"', true, 3);
    index.reach(&path, 4, &Cancel::default()).unwrap();
    // Record 3 marked (it starts the second group of 3), not much more read.
    assert_eq!(index.mark_for(4), Some((5 + 3 * 5, 3)));
    assert!(!index.done() && index.records() == 4);
    assert_eq!(index.mark_for(9), None);
    assert_eq!(index.complete(&path, &Cancel::default()), Ok(10));
    assert_eq!(index.marks(), 4);
    assert_eq!(index.mark_for(9), Some((5 + 9 * 5, 9)));
    assert_eq!(index.scanned(), (bytes.len() as u64, hash(HASH_START, &bytes[3..])));
}
