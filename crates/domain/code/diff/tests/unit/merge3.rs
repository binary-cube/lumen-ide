use super::*;

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

fn kinds(base: &str, ours: &str, theirs: &str) -> Vec<RegionKind> {
    merge3(&lines(base), &lines(ours), &lines(theirs)).into_iter().map(|r| r.kind).collect()
}

#[test]
fn a_change_on_one_side_only_is_taken_from_it() {
    let (base, ours, theirs) = ("a\nb\nc\nd\ne", "a\nB\nc\nd\ne", "a\nb\nc\nd\nE");
    let regions = merge3(&lines(base), &lines(ours), &lines(theirs));
    assert_eq!(
        regions.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [RegionKind::Equal, RegionKind::Ours, RegionKind::Equal, RegionKind::Theirs]
    );
    assert_eq!(regions[1], Region { kind: RegionKind::Ours, base: 1..2, ours: 1..2, theirs: 1..2 });
    assert_eq!(regions[3].theirs, 4..5);
    let merged = MergeChunks::of(&regions);
    assert_eq!(merged.picks, [Pick::Left, Pick::Right]);
    assert_eq!(merged.conflict_count(), 0);
    let result = compose(&lines(ours), &lines(theirs), &merged.chunks, &merged.picks);
    assert_eq!(result, ["a", "B", "c", "d", "E"]);
}

#[test]
fn changes_on_both_sides_conflict_unless_the_same() {
    assert_eq!(
        kinds("a\nb\nc", "a\nX\nc", "a\nY\nc"),
        [RegionKind::Equal, RegionKind::Conflict, RegionKind::Equal]
    );
    assert_eq!(
        kinds("a\nb\nc", "a\nX\nc", "a\nX\nc"),
        [RegionKind::Equal, RegionKind::Same, RegionKind::Equal]
    );
    // Touching changes (one right after the other's) conflict too, as git has them.
    assert_eq!(
        kinds("a\nb\nc\nd", "a\nB\nc\nd", "a\nb\nC\nd"),
        [RegionKind::Equal, RegionKind::Conflict, RegionKind::Equal]
    );
    let merged = MergeChunks::of(&merge3(&lines("a\nb\nc"), &lines("a\nX\nc"), &lines("a\nY\nc")));
    assert_eq!(
        (merged.picks.as_slice(), merged.conflicts.as_slice()),
        ([Pick::None].as_slice(), [true].as_slice())
    );
}

#[test]
fn lines_added_and_removed_keep_the_sides_aligned() {
    // Ours adds two lines at the top; theirs removes one near the end.
    let (base, ours, theirs) = ("a\nb\nc\nd", "x\ny\na\nb\nc\nd", "a\nb\nd");
    let regions = merge3(&lines(base), &lines(ours), &lines(theirs));
    let merged = MergeChunks::of(&regions);
    assert_eq!(merged.conflict_count(), 0);
    let result = compose(&lines(ours), &lines(theirs), &merged.chunks, &merged.picks);
    assert_eq!(result, ["x", "y", "a", "b", "d"]);
    // Every region's ranges follow on from the last one's, on each side.
    for pair in regions.windows(2) {
        assert_eq!(pair[0].ours.end, pair[1].ours.start);
        assert_eq!(pair[0].theirs.end, pair[1].theirs.start);
        assert_eq!(pair[0].base.end, pair[1].base.start);
    }
}

#[test]
fn both_adding_the_file_or_nothing_changing_is_one_region() {
    assert_eq!(kinds("", "a", "b"), [RegionKind::Conflict]);
    assert_eq!(kinds("a\nb", "a\nb", "a\nb"), [RegionKind::Equal]);
    assert!(merge3::<&str>(&[], &[], &[]).iter().all(|r| r.kind == RegionKind::Equal));
}
