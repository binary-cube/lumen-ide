use super::*;

fn stamp(day: Date, path: &str) -> Stamp {
    Stamp { day, time: "10:00".into(), lower: path.to_lowercase() }
}

/// Each day once, before its files, with how many of them are shown.
#[test]
fn the_files_come_under_their_days() {
    let (a, b) = (Date::new(2026, 11, 15).unwrap(), Date::new(2026, 11, 14).unwrap());
    let stamps = [stamp(a, "x"), stamp(a, "y"), stamp(b, "z"), stamp(b, "w")];
    assert_eq!(
        items(&[0, 1, 2, 3], &stamps),
        [
            Item::Day { day: a, count: 2 },
            Item::File(0),
            Item::File(1),
            Item::Day { day: b, count: 2 },
            Item::File(2),
            Item::File(3),
        ]
    );
    // Only what's shown: a day without any isn't there.
    assert_eq!(items(&[2], &stamps), [Item::Day { day: b, count: 1 }, Item::File(2)]);
    assert!(items(&[], &stamps).is_empty());
}

/// Every word typed, in any case, somewhere in the path.
#[test]
fn a_search_needs_every_word_in_the_path() {
    let path = "admin/Draw/body.categorii.php".to_lowercase();
    assert!(matches(&path, &words("")));
    assert!(matches(&path, &words("DRAW categ")));
    assert!(matches(&path, &words("  .php  ")));
    assert!(!matches(&path, &words("draw css")));
}

/// Each day but the first has air above it; the list a margin above its first row; a point
/// finds its row, none in the margins.
#[test]
fn the_rows_have_their_places() {
    let (a, b) = (Date::new(2026, 11, 15).unwrap(), Date::new(2026, 11, 14).unwrap());
    let list = [
        Item::Day { day: a, count: 1 },
        Item::File(0),
        Item::Day { day: b, count: 1 },
        Item::File(1),
    ];
    let tops = tops(&list);
    assert_eq!(tops.len(), 5);
    assert_eq!(tops[0], PAD_TOP);
    assert_eq!(tops[1], PAD_TOP + DAY_H);
    assert_eq!(tops[3] - tops[2], DAY_GAP + DAY_H, "air above the second day");
    assert_eq!(tops[4] - tops[3], FILE_H);
    assert_eq!(item_at(&tops, 0.0), None, "the margin above");
    assert_eq!(item_at(&tops, PAD_TOP), Some(0));
    assert_eq!(item_at(&tops, tops[1] + 1.0), Some(1));
    assert_eq!(item_at(&tops, tops[3] + FILE_H - 1.0), Some(3));
    assert_eq!(item_at(&tops, tops[4]), None, "under the last");
    assert_eq!(item_at(&[], 3.0), None);
}

/// Nothing pinned until a day's heading goes above the top; then that day, until the next
/// one's heading reaches it and pushes it out.
#[test]
fn the_day_scrolled_through_stays_at_the_top() {
    let (a, b) = (Date::new(2026, 11, 15).unwrap(), Date::new(2026, 11, 14).unwrap());
    let list = [
        Item::Day { day: a, count: 2 },
        Item::File(0),
        Item::File(1),
        Item::Day { day: b, count: 1 },
        Item::File(2),
    ];
    let tops = tops(&list);
    assert_eq!(pinned(&list, &tops, 0.0), None, "the first heading is in view");
    let pin = |scroll| pinned(&list, &tops, scroll);
    assert_eq!(pin(PAD_TOP + 1.0), Some(Pinned { day: 0, pushed: 0.0 }));
    // The second heading (under its air) reaching the pinned one: pushed by as much.
    let second = tops[4] - DAY_H;
    assert_eq!(pin(second - DAY_H + 5.0), Some(Pinned { day: 0, pushed: 5.0 }));
    assert_eq!(pin(second + 1.0), Some(Pinned { day: 3, pushed: 0.0 }));
}
