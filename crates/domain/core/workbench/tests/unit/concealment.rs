#![allow(clippy::single_range_in_vec_init)]

use super::*;

#[test]
fn masks_only_the_hidden_bytes_of_the_line() {
    let mut line = "APP_SECRET=s3cr3t # note".to_owned();
    // The line starts at byte 100 of the text; the value is at 111..117.
    mask(&mut line, 100, &[111..117]);
    assert_eq!(line, "APP_SECRET=****** # note");
}

#[test]
fn keeps_every_byte_in_place() {
    let mut line = "K=ăb€c".to_owned();
    let before = line.len();
    mask(&mut line, 0, &[2..before]);
    assert_eq!(line, "K=·*•*");
    assert_eq!(line.len(), before);
}

#[test]
fn keeps_whitespace_and_what_lies_outside_the_line() {
    let mut line = "\tab cd".to_owned();
    // A range starting on the line before and one ending on the line after.
    mask(&mut line, 10, &[0..12, 14..40]);
    assert_eq!(line, "\t*b **");
}

#[test]
fn nothing_hidden_leaves_the_line_alone() {
    let mut line = "KEY=value".to_owned();
    mask(&mut line, 0, &[]);
    mask(&mut line, 0, &[20..30]);
    assert_eq!(line, "KEY=value");
}

#[test]
fn revealed_ranges_are_shown() {
    let concealed = [0..3, 5..8, 10..12];
    assert_eq!(hidden(&concealed, |r| r.contains(&6)), vec![0..3, 10..12]);
    assert_eq!(hidden(&concealed, |_| false), concealed.to_vec());
}
