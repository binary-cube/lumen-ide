use super::*;
use kernel::Rgba;

fn theme() -> Theme {
    Theme {
        canvas: Rgba::rgb(0x1e1e1e),
        fill: Rgba::rgb(0x2d2d2d),
        stroke: Rgba::rgb(0x999999),
        text: Rgba::rgb(0xeeeeee),
    }
}

/// The sections in the order chosen; every shape's style read as a shape (or a text) drawn.
#[test]
fn the_catalog_s_sections_and_shapes() {
    let ids: Vec<&str> = SECTIONS.iter().map(|s| s.id).collect();
    assert_eq!(ids, ["general", "basic", "flowchart", "entity_relation", "uml", "arrows"]);
    for section in SECTIONS {
        for e in section.entries {
            assert!(!thumbnail(e, &theme(), 2.0).is_empty(), "{}.{}: drawn", section.id, e.id);
        }
    }
}

/// A search finds by title (case aside) across the sections; empty: everything.
#[test]
fn a_search_finds_by_title() {
    let title = |_: &Section, e: &Entry| e.id.replace('_', " ");
    let found = search("CYL", title);
    assert_eq!(found, [(0, 11)]);
    assert_eq!(search("", title).len(), SECTIONS.iter().map(|s| s.entries.len()).sum::<usize>());
    assert!(search("process", title).len() >= 2, "in two sections");
}

/// Without a waker, a section's thumbnails are made at once; another theme: made again.
#[test]
fn thumbnails_are_made_per_theme_and_scale() {
    let mut thumbs = Thumbnails::default();
    let n = thumbs.section(0, &theme(), 1.0, None).map(<[_]>::len);
    assert_eq!(n, Some(SECTIONS[0].entries.len()));
    assert!(thumbs.bytes() > 0);
    let other = Theme { fill: Rgba::rgb(0xffffff), ..theme() };
    assert!(thumbs.section(1, &other, 1.0, None).is_some());
    assert!(thumbs.made.len() == 1, "the other theme's let go");
}
