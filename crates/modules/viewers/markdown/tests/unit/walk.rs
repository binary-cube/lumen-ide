use std::collections::HashSet;
use std::path::Path;

use text_layout::TextSystem;
use ui_components::Palette;

use crate::layout::{Hit, Item, Layout, Page};

fn build(source: &str, toggled: &HashSet<usize>) -> Layout {
    let mut images = crate::layout::Caches::inline();
    let mut text = TextSystem::new();
    let page = Page { source, version: 1, width: 800.0, scale: 1.0, dir: Path::new("."), toggled };
    Layout::build(page, &mut images, &mut text, &Palette::default())
}

fn texts(layout: &Layout) -> Vec<String> {
    layout
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.trim().to_owned()),
            _ => None,
        })
        .collect()
}

fn joined(layout: &Layout) -> String {
    texts(layout).join(" ")
}

#[test]
fn alerts_have_their_title_and_icon() {
    let l = build("> [!WARNING]\n> Careful here.\n", &HashSet::new());
    assert!(l.items.iter().any(|i| matches!(i, Item::Icon { name: "alert", .. })));
    let t = joined(&l);
    assert!(!t.contains("[!WARNING]"), "{t}");
    assert!(t.contains("Careful"), "{t}");
}

#[test]
fn callouts_fold_and_open() {
    let source = "> [!tip]- Hidden tip\n> The content.\n";
    let closed = build(source, &HashSet::new());
    let t = joined(&closed);
    assert!(t.contains("Hidden") && !t.contains("content"), "{t}");
    let fold = closed.hits.iter().find_map(|(_, h)| match h {
        Hit::Fold(i) => Some(*i),
        _ => None,
    });
    let open = build(source, &HashSet::from([fold.expect("a fold")]));
    assert!(joined(&open).contains("content"));
}

#[test]
fn details_fold() {
    let source = "<details>\n<summary>More</summary>\n\nInside **text**\n\n</details>\n\nAfter\n";
    let closed = build(source, &HashSet::new());
    let t = joined(&closed);
    assert!(t.contains("More") && t.contains("After") && !t.contains("Inside"), "{t}");
    let open = build(source, &HashSet::from([0]));
    assert!(joined(&open).contains("Inside"));
}

#[test]
fn footnotes_are_numbered_and_gathered_at_the_end() {
    let source = "Text[^b] and more[^a].\n\n[^a]: First defined.\n[^b]: Second defined.\n\nLast paragraph.\n";
    let l = build(source, &HashSet::new());
    let t = texts(&l);
    let last = t.iter().position(|s| s == "paragraph.").expect("last paragraph");
    let second = t.iter().position(|s| s == "Second").expect("definition");
    assert!(second > last, "definitions at the end: {t:?}");
    // "b" is referenced first: number 1, listed first.
    let first = t.iter().position(|s| s == "First").expect("definition");
    assert!(second < first, "{t:?}");
    assert!(l.anchor("fn-b").is_some() && l.anchor("fnref-a").is_some());
    assert!(l.hits.iter().any(|(_, h)| *h == Hit::Link("#fn-b".into())));
}

#[test]
fn headings_have_anchors_and_a_table_of_contents() {
    let l = build("[TOC]\n\n# One Thing\n\n## Two, too\n\n## Two, too\n", &HashSet::new());
    for anchor in ["one-thing", "two-too", "two-too-1"] {
        assert!(l.anchor(anchor).is_some(), "{anchor}");
        assert!(l.hits.iter().any(|(_, h)| *h == Hit::Link(format!("#{anchor}"))), "{anchor}");
    }
}

#[test]
fn task_lists_tick_their_source() {
    let source = "- [ ] open\n- [x] done\n";
    let l = build(source, &HashSet::new());
    let tasks: Vec<_> = l
        .hits
        .iter()
        .filter_map(|(_, h)| match h {
            Hit::Task(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(tasks.len(), 2);
    let ticked = crate::toggle_task(source, tasks[0].clone()).expect("ticked");
    assert_eq!(ticked, "- [x] open\n- [x] done\n");
    let unticked = crate::toggle_task(source, tasks[1].clone()).expect("unticked");
    assert_eq!(unticked, "- [ ] open\n- [ ] done\n");
}

#[test]
fn autolinks_emoji_marks_and_wikilinks() {
    let l = build("Go to https://example.com :rocket: ==now== [[Other Page]]\n", &HashSet::new());
    let t = joined(&l);
    assert!(t.contains("🚀") && t.contains("now") && !t.contains("=="), "{t}");
    assert!(l.hits.iter().any(|(_, h)| *h == Hit::Link("https://example.com".into())));
    assert!(l.hits.iter().any(|(_, h)| *h == Hit::Link("Other Page.md".into())));
}

#[test]
fn front_matter_is_a_table() {
    let l = build("---\ntitle: Hello\ntags:\n  - a\n  - b\n---\n\nBody\n", &HashSet::new());
    let t = joined(&l);
    assert!(t.contains("title") && t.contains("Hello") && t.contains("a, b"), "{t}");
    assert!(!t.contains("---"));
}

#[test]
fn html_is_read() {
    let l = build(
        "<p align=\"center\"><b>Bold</b> <kbd>Ctrl</kbd> H<sub>2</sub>O</p>\n\n<!-- hidden -->\n",
        &HashSet::new(),
    );
    let t = joined(&l);
    assert!(
        t.contains("Bold") && t.contains("Ctrl") && !t.contains("hidden") && !t.contains('<'),
        "{t}"
    );
}

#[test]
fn tables_fit_their_content_and_align() {
    let l = build("| a | b |\n|:-|-:|\n| x | long cell text |\n", &HashSet::new());
    let x_of = |word: &str| {
        l.items.iter().find_map(|i| match i {
            Item::Text { text, x, .. } if text.trim() == word => Some(*x),
            _ => None,
        })
    };
    // A narrow table (its content's width), the second column right-aligned.
    let (b, text) = (x_of("b").expect("b"), x_of("text").expect("text"));
    assert!(b > text, "b right-aligned: {b} vs {text}");
    let border = l.items.iter().find_map(|i| match i {
        Item::Border { rect, .. } => Some(*rect),
        _ => None,
    });
    assert!(border.expect("frame").size.width < 500.0);
}

#[test]
fn code_is_colored_scrolled_or_wrapped_and_copyable() {
    let long = "x".repeat(400);
    let source = format!("```php\n$a = \"s\"; // c\n{long}\n```\n");
    let l = build(&source, &HashSet::new());
    assert_eq!(l.code_blocks.len(), 1);
    // A line wider than the block: its text scrolls across, clipped to the block.
    let Some(Item::Scroll { content, room, items, .. }) =
        l.items.iter().find(|i| matches!(i, Item::Scroll { .. }))
    else {
        panic!("a block scrolling across");
    };
    assert!(content > room);
    let colors: HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            Item::Text { color, font, .. } if font.family == Palette::default().code_font => {
                Some(format!("{color:?}"))
            }
            _ => None,
        })
        .collect();
    // (The languages' keywords come from their modules: here only strings stand out.)
    assert!(colors.len() >= 2, "{colors:?}");
    // Wrapped (the setting): within the block, nothing to scroll.
    crate::settings::set_wrap_code(true);
    let l = build(&source, &HashSet::new());
    crate::settings::set_wrap_code(false);
    let block = l.code_blocks[0].0;
    assert!(!l.items.iter().any(|i| matches!(i, Item::Scroll { .. })));
    assert!(l.items.iter().all(|i| match i {
        Item::Text { x, .. } => *x <= block.max_x(),
        _ => true,
    }));
}

/// Marks every `fn` as a keyword.
struct FakeHighlighter;

impl ui_components::snippets::SnippetHighlighter for FakeHighlighter {
    fn highlight(
        &self,
        extension: &str,
        code: &str,
    ) -> Option<Vec<(std::ops::Range<usize>, std::sync::Arc<str>)>> {
        (extension == "rs").then(|| {
            code.match_indices("fn").map(|(i, _)| (i..i + 2, "keyword.function".into())).collect()
        })
    }
}

#[test]
fn code_is_highlighted_by_the_grammar_off_the_painting_thread() {
    ui_components::snippets::register(std::sync::Arc::new(FakeHighlighter));
    let mut palette = Palette::default();
    let keyword = kernel::Rgba::rgb(0x123456);
    let base = palette.code_text;
    palette.syntax = Some(ui_components::SyntaxColors {
        keyword,
        variable: base,
        string: base,
        number: base,
        function: base,
        comment: base,
        type_name: base,
        constant: base,
        property: base,
        tag: base,
        attribute: base,
        operator: base,
        punctuation: base,
    });
    let source = "```rust\n\tfn main() {}\n```\n";
    let mut caches = crate::layout::Caches::default();
    let mut text = TextSystem::new();
    let toggled = HashSet::new();
    let page = || Page {
        source,
        version: 1,
        width: 800.0,
        scale: 1.0,
        dir: Path::new("."),
        toggled: &toggled,
    };
    let colored = |l: &Layout| {
        l.items.iter().any(|i| matches!(i, Item::Text { text, color, .. } if text.trim() == "fn" && *color == keyword))
    };
    let first = Layout::build(page(), &mut caches, &mut text, &palette);
    assert!(!colored(&first), "not yet: worked out in the background");
    let started = std::time::Instant::now();
    while !caches.poll() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let second = Layout::build(page(), &mut caches, &mut text, &palette);
    assert!(colored(&second));
}

#[test]
fn math_inline_and_displayed() {
    let l = build(
        "Euler: $e^{i\\pi} + 1 = 0$ and\n\n$$\\int_0^1 x\\,dx = \\frac{1}{2}$$\n\n```math\n\\sqrt{2}\n```\n",
        &HashSet::new(),
    );
    let t = joined(&l);
    assert!(!t.contains('$'), "{t}");
    assert!(t.contains("Euler"));
    let bad = build("Broken $\\frac{a$ here", &HashSet::new());
    assert!(joined(&bad).contains("\\frac{a"), "{}", joined(&bad));
}
