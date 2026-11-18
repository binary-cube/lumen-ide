use super::*;
use crate::palette::Palette;

fn notes() -> ReadText {
    ReadText::new(vec![
        Block::new(BlockKind::Heading, "What's new"),
        Block::new(BlockKind::Gap, ""),
        Block::new(BlockKind::Point { depth: 0 }, "Tabs keep their order"),
        Block::new(BlockKind::Point { depth: 1 }, "even after a restart"),
        Block::new(BlockKind::Paragraph, "Thanks to everyone who tried the betas."),
    ])
}

fn painted(read: &mut ReadText, text: &mut TextSystem, area: Rect) {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    read.paint(&mut Scene::default(), text, &ctx, area);
}

/// Laid out once per width: each block's rows, the heading taller, the gap kept.
#[test]
fn it_is_laid_out_once_per_width() {
    let mut text = TextSystem::new();
    let mut read = notes();
    painted(&mut read, &mut text, Rect::new(0.0, 0.0, 600.0, 400.0));
    let rows = read.laid.as_ref().unwrap().rows.len();
    assert_eq!(rows, 4, "one row a block (the gap none)");
    assert_eq!(read.content_height(), HEADING_LINE + GAP + 3.0 * BODY_LINE);
    // Narrow: the long paragraph wraps at words, every byte in one row or a space between.
    painted(&mut read, &mut text, Rect::new(0.0, 0.0, 120.0, 400.0));
    let laid = read.laid.as_ref().unwrap();
    let para: Vec<&Row> = laid.rows.iter().filter(|r| r.block == 4).collect();
    assert!(para.len() > 1, "wrapped");
    let joined: Vec<&str> = para.iter().map(|r| &read.blocks[4].text[r.range.clone()]).collect();
    assert_eq!(joined.join(" "), read.blocks[4].text);
}

/// A press and a drag select across blocks; ⇧ extends; the copy keeps the points' bullets.
#[test]
fn a_drag_selects_across_blocks_and_copies_as_text() {
    let mut text = TextSystem::new();
    let mut read = notes();
    let area = Rect::new(0.0, 0.0, 600.0, 400.0);
    painted(&mut read, &mut text, area);
    assert_eq!(read.selected_text(), None);
    // From the heading's start to the second point's end.
    assert!(read.press(Point::new(1.0, 5.0), 1, false));
    let second_row = read.laid.as_ref().unwrap().rows[2].clone();
    let end = Point::new(590.0, second_row.y + 5.0);
    assert!(read.drag(end));
    assert!(read.release());
    assert_eq!(
        read.selected_text().as_deref(),
        Some("What's new\n\n• Tabs keep their order\n  • even after a restart")
    );
    // A double click: its word; a triple: its block.
    let first_point = read.laid.as_ref().unwrap().rows[1].clone();
    let x = first_point.x + first_point.shaped.x_at(1);
    read.press(Point::new(x, first_point.y + 5.0), 2, false);
    assert_eq!(read.selected_text().as_deref(), Some("• Tabs"));
    read.press(Point::new(x, first_point.y + 5.0), 3, false);
    assert_eq!(read.selected_text().as_deref(), Some("• Tabs keep their order"));
    assert!(read.select_all());
    assert!(read.selected_text().unwrap().ends_with("tried the betas."));
    assert!(read.clear_selection() && read.selected_text().is_none());
}

/// Taller than its area: the wheel scrolls it within its content, and the bar's thumb
/// dragged; outside it, nothing taken.
#[test]
fn a_long_text_scrolls_by_the_wheel_and_its_bar() {
    let mut text = TextSystem::new();
    let blocks =
        (0..60).map(|i| Block::new(BlockKind::Point { depth: 0 }, format!("Point {i}"))).collect();
    let mut read = ReadText::new(blocks);
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    painted(&mut read, &mut text, area);
    let max = read.content_height() - area.size.height;
    assert!(read.scroll_by(50.0) && read.scroll() == 50.0);
    assert!(read.scroll_by(1e6) && read.scroll() == max, "never past its end");
    assert!(read.scroll_by(-1e6) && read.scroll() == 0.0);
    painted(&mut read, &mut text, area);
    // The thumb, at the top of the bar on the right edge: taken and dragged down.
    let thumb = Point::new(area.max_x() - 3.0, 5.0);
    assert!(!read.over_text(thumb), "the bar's arrow, not the I-beam");
    assert!(read.press(thumb, 1, false));
    assert!(read.drag(Point::new(thumb.x, 100.0)));
    assert!(read.scroll() > 0.0, "dragged");
    assert!(read.release());
    assert!(!read.press(Point::new(500.0, 500.0), 1, false), "outside it");
}
