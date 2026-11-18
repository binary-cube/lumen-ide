use super::*;

fn numbered(lines: std::ops::Range<u32>) -> String {
    lines.map(|i| format!("line {i}\n")).collect()
}

#[test]
fn changes_far_apart_are_two_hunks_each_with_its_context() {
    let old = numbered(1..31);
    let new = old.replace("line 5\n", "FIVE\n").replace("line 25\n", "TWENTY-FIVE\n");
    let found = hunks(&old, &new, 3);
    assert_eq!(found.len(), 2);
    assert_eq!((found[0].old_start, found[0].old_count), (1, 7));
    assert_eq!(found[0].new_lines(), (2, 8));
    let kinds: Vec<LineKind> = found[0].lines.iter().map(|l| l.kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == LineKind::Removed).count(), 1);
    assert_eq!(found[0].lines[3].old, Some(5));
    assert_eq!(found[0].lines[4].new, Some(5));
}

#[test]
fn changes_close_together_are_one_hunk() {
    let old = numbered(1..21);
    let new = old.replace("line 5\n", "A\n").replace("line 9\n", "B\n");
    assert_eq!(hunks(&old, &new, 3).len(), 1);
}

#[test]
fn a_hunks_patch_is_a_unified_diff() {
    let found = hunks("one\ntwo\n", "one\nTWO\n", 3);
    assert_eq!(
        found[0].patch("a b.txt"),
        "--- a/a b.txt\n+++ b/a b.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n"
    );
}

#[test]
fn a_last_line_without_its_ending_is_said() {
    let patch = hunks("one", "two", 0)[0].patch("f");
    assert!(patch.contains("-one\n\\ No newline at end of file\n+two\n\\ No newline"), "{patch}");
}

#[test]
fn a_new_file_counts_from_nothing() {
    let found = hunks("", "a\nb\n", 3);
    assert_eq!(found[0].patch("f"), "--- a/f\n+++ b/f\n@@ -0,0 +1,2 @@\n+a\n+b\n");
}

#[test]
fn a_hunk_only_removing_is_named_by_the_lines_it_removes() {
    assert_eq!(hunks("a\nb\n", "", 3)[0].shown_lines(), (1, 2));
    assert_eq!(hunks("a\n", "a\nb\n", 0)[0].shown_lines(), (2, 2));
}

#[test]
fn nothing_changed_is_no_hunk() {
    assert!(hunks("same\n", "same\n", 3).is_empty());
}

#[test]
fn a_patch_of_some_lines_leaves_the_others_as_the_side_it_applies_to_has_them() {
    let found = hunks("a\nb\nc\n", "a\nB\nC\n", 0);
    let hunk = &found[0];
    assert_eq!(hunk.lines.len(), 4, "-b -c +B +C");
    // Forward, only -b and +B: c stays where the old side has it, +C isn't there.
    let some = |i: usize| i == 0 || i == 2;
    assert_eq!(
        hunk.patch_lines("f", some, false),
        "--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n-b\n+B\n c\n"
    );
    // Taken back: C stays as the new side has it, -c isn't there.
    assert_eq!(
        hunk.patch_lines("f", some, true),
        "--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n-b\n+B\n C\n"
    );
}

#[test]
fn the_whole_hunk_is_the_patch_of_all_its_lines() {
    let found = hunks("one\ntwo\n", "one\nTWO\n", 3);
    assert_eq!(found[0].patch("f"), found[0].patch_lines("f", |_| true, true));
}

#[test]
fn the_second_change_of_a_run_keeps_the_first_line_before_it() {
    let found = hunks("a\nb\nc\n", "a\nB\nC\n", 0);
    let some = |i: usize| i == 1 || i == 3;
    let forward = found[0].patch_lines("f", some, false);
    assert!(forward.ends_with(" b\n-c\n+C\n"), "{forward}");
    let back = found[0].patch_lines("f", some, true);
    assert!(back.ends_with(" B\n-c\n+C\n"), "{back}");
}
