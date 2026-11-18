use super::*;
use crate::OperationKind;

const TWO_WAY: &str = "\
<?php
<<<<<<< HEAD
$a = 1;
=======
$a = 2;
$b = 3;
>>>>>>> 275df80 (Cart fix)
echo $a;
";

#[test]
fn the_markers_are_read_into_hunks_and_written_back_as_they_were() {
    let merged = Merged::parse(TWO_WAY);
    assert_eq!(merged.unresolved(), 1);
    let hunk = merged.hunks().next().unwrap();
    assert_eq!(hunk.ours, "$a = 1;\n");
    assert_eq!(hunk.theirs, "$a = 2;\n$b = 3;\n");
    assert_eq!(hunk.base, None);
    assert_eq!(
        (hunk.ours_label.as_str(), hunk.theirs_label.as_str()),
        ("HEAD", "275df80 (Cart fix)")
    );
    assert_eq!(merged.text(), TWO_WAY);
    assert_eq!(merged.hunk_lines(0), Some(1..7));
    assert_eq!(merged.hunk_lines(1), None);
}

#[test]
fn each_choice_resolves_a_hunk() {
    let hunk = Merged::parse(TWO_WAY).hunks().next().unwrap().clone();
    assert_eq!(hunk.resolved(&Choice::Ours), "$a = 1;\n");
    assert_eq!(hunk.resolved(&Choice::OursThenTheirs), "$a = 1;\n$a = 2;\n$b = 3;\n");
    assert_eq!(hunk.resolved(&Choice::TheirsThenOurs), "$a = 2;\n$b = 3;\n$a = 1;\n");
    assert_eq!(hunk.resolved(&Choice::Base), "");
    assert_eq!(hunk.resolved(&Choice::Text("$a = 3;\n".into())), "$a = 3;\n");

    let mut merged = Merged::parse(TWO_WAY);
    assert!(merged.resolve(0, &Choice::Theirs));
    assert!(merged.is_resolved());
    assert!(!merged.resolve(0, &Choice::Ours), "nothing left to resolve");
    assert_eq!(merged.text(), "<?php\n$a = 2;\n$b = 3;\necho $a;\n");
}

#[test]
fn diff3_markers_keep_the_base() {
    let text = "a\n<<<<<<< ours\nx\n||||||| base\nb\n=======\ny\n>>>>>>> theirs\nz\n<<<<<<< ours\n1\n=======\n2\n>>>>>>> theirs\n";
    let mut merged = Merged::parse(text);
    assert_eq!(merged.unresolved(), 2);
    let first = merged.hunks().next().unwrap();
    assert_eq!((first.base.as_deref(), first.base_label.as_str()), (Some("b\n"), "base"));
    assert_eq!(merged.text(), text);
    assert_eq!(merged.hunk_lines(1), Some(9..14));
    assert_eq!(merged.side(Side::Ours), "a\nx\nz\n1\n");
    assert_eq!(merged.side(Side::Theirs), "a\ny\nz\n2\n");
    // Resolving the first: the second's lines move up.
    merged.resolve(0, &Choice::Base);
    assert_eq!(merged.hunk_lines(0), Some(3..8));
    assert_eq!(merged.text(), "a\nb\nz\n<<<<<<< ours\n1\n=======\n2\n>>>>>>> theirs\n");
}

#[test]
fn text_that_only_looks_like_markers_stays_text() {
    for text in [
        "<<<<<<< HEAD\nnever closed\n=======\n",
        "=======\nalone\n>>>>>>> x\n",
        "<<<<<<<< eight\n=======\n>>>>>>> x\n",
    ] {
        let merged = Merged::parse(text);
        assert!(merged.is_resolved(), "{text:?}");
        assert_eq!(merged.text(), text);
    }
    // An opening marker never closed before another hunk: only the inner one is a hunk.
    let text = "<<<<<<< a\n<<<<<<< b\n=======\n>>>>>>> x\n";
    let merged = Merged::parse(text);
    assert_eq!(merged.pieces()[0], Piece::Common("<<<<<<< a\n".into()));
    assert_eq!((merged.unresolved(), merged.text().as_str()), (1, text));
}

#[test]
fn windows_line_endings_are_kept_and_a_last_line_without_one_is_fine() {
    let text = "<<<<<<< HEAD\r\na\r\n=======\r\nb\r\n>>>>>>> x\r\nend";
    let mut merged = Merged::parse(text);
    assert_eq!(merged.unresolved(), 1);
    assert_eq!(merged.text(), text);
    merged.resolve(0, &Choice::OursThenTheirs);
    assert_eq!(merged.text(), "a\r\nb\r\nend");
    // Hand-written lines without a last line ending still leave the marker on its own line.
    let mut merged = Merged::parse(
        "<<<<<<< HEAD\na\n=======\nb\n>>>>>>> x\n<<<<<<< HEAD\nc\n=======\nd\n>>>>>>> x\n",
    );
    merged.resolve(0, &Choice::Text("joined".into()));
    assert_eq!(merged.unresolved(), 1);
    assert!(merged.text().starts_with("joined"));
}

#[test]
fn each_operation_says_what_can_be_done_with_it() {
    use OperationStep::{Abort, Continue, Skip};
    for kind in [OperationKind::Rebase, OperationKind::CherryPick, OperationKind::Revert] {
        assert!([Continue, Skip, Abort].iter().all(|s| kind.allows(*s)), "{kind:?}");
    }
    assert!(OperationKind::Merge.allows(Continue) && !OperationKind::Merge.allows(Skip));
    assert!(OperationKind::Bisect.allows(Abort) && !OperationKind::Bisect.allows(Continue));
    assert!(!OperationKind::Bisect.conflicts() && OperationKind::Merge.conflicts());
}
