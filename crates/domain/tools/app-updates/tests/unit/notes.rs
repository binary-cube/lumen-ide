use super::*;

fn kinds(notes: &str) -> Vec<(Kind, String)> {
    lines(notes).into_iter().map(|l| (l.kind, l.text)).collect()
}

/// Titles, points and paragraphs, one gap between blocks, the marks taken off.
#[test]
fn notes_become_titles_points_and_paragraphs() {
    let notes = "## What's new\n\nThe **first** beta:\nfast and `native`.\n\n- [Git](https://x) log\n  - blame too\n1. one\n\n---\n# Download\n![shot](a.png)Get it.";
    assert_eq!(
        kinds(notes),
        [
            (Kind::Title, "What's new".to_owned()),
            (Kind::Gap, String::new()),
            (Kind::Text, "The first beta: fast and native.".to_owned()),
            (Kind::Gap, String::new()),
            (Kind::Point { depth: 0 }, "Git log".to_owned()),
            (Kind::Point { depth: 1 }, "blame too".to_owned()),
            (Kind::Point { depth: 0 }, "one".to_owned()),
            (Kind::Gap, String::new()),
            (Kind::Title, "Download".to_owned()),
            (Kind::Gap, String::new()),
            (Kind::Text, "Get it.".to_owned()),
        ]
    );
}

/// Nothing written: no lines; a word that only looks like a mark stays; text outside ASCII
/// is never cut inside a character.
#[test]
fn odd_notes_stay_readable() {
    assert!(lines("").is_empty() && lines("\n\n").is_empty());
    assert_eq!(plain("a [b c"), "a [b c");
    assert_eq!(plain("#hashtag ăîș `ț`"), "#hashtag ăîș ț");
    assert_eq!(kinds("#no-title"), [(Kind::Text, "#no-title".to_owned())]);
}

/// The notes written for the app (between the comments) are what it shows; without them,
/// everything but the download section.
#[test]
fn the_app_shows_its_own_part_of_the_notes() {
    let both =
        "Shown on GitHub.\n<!-- app -->\n## New\n- tabs\n<!-- /app -->\n## Download\n- a.zip";
    assert_eq!(for_app(both), "## New\n- tabs");
    let plain = "## What's new\n- tabs\n## Download\n- a.zip\n### Sums\nx\n## Thanks\nall";
    assert_eq!(for_app(plain), "## What's new\n- tabs\n## Thanks\nall");
    assert_eq!(for_app("<!-- app -->only this"), "only this", "an end missing: to the end");
}
