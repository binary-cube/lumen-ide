use super::*;

#[test]
fn waits_for_the_delay_and_follows_the_word() {
    let mut hover = CodeHover::default();
    assert!(!hover.track(Some(3..8), Point::new(1.0, 1.0), false));
    assert!(hover.due().is_none(), "not yet");
    // Same word: the wait goes on (not restarted).
    hover.track(Some(3..8), Point::new(2.0, 1.0), false);
    hover.pending.as_mut().unwrap().2 -= DELAY;
    assert_eq!(hover.due().map(|(w, _)| w), Some(3..8));
    hover.show(Some(HoverCard { code: "strlen()".into(), ..Default::default() }));
    assert!(hover.shown.is_some() && hover.due().is_none());
    // Another word: the card goes away at once.
    assert!(hover.track(Some(10..12), Point::new(9.0, 1.0), false));
    assert!(hover.shown.is_none());
    // The mouse moving onto the card (no word under it) keeps the card.
    hover.track(Some(30..34), Point::new(10.0, 10.0), true);
    hover.show(Some(HoverCard::default()));
    if let Some(shown) = &mut hover.shown {
        shown.rect = Rect::new(0.0, 30.0, 200.0, 100.0);
    }
    assert!(!hover.track(None, Point::new(50.0, 20.0), false), "on the way to it");
    assert!(!hover.track(None, Point::new(50.0, 80.0), false), "on it");
    assert!(hover.contains(Point::new(50.0, 80.0)));
    assert!(hover.track(None, Point::new(500.0, 80.0), false), "away: hidden");
    // ⌘ held: due at once.
    hover.track(Some(20..24), Point::new(9.0, 1.0), true);
    assert!(hover.due().is_some());
}

/// A word wider than a line (HTML a server didn't render, a long URL) is broken at the
/// edge: no line is wider than the room.
#[test]
fn a_word_too_wide_is_broken_at_the_edge() {
    let mut ts = TextSystem::new();
    let palette = Palette::dark();
    let font = Font::new(palette.ui_font.clone(), palette.ui_font_size);
    let word = "valign=\"top\"><td>INFO_CONFIGURATION</td><td>4</td><td>Current";
    let lines = flow(&format!("short {word} end"), 120.0, 120.0, &font, &mut ts);
    assert!(lines.len() > 3, "{lines:?}");
    for line in &lines {
        assert!(ts.shape_line(line, &font).width <= 120.0, "{line:?}");
    }
    assert_eq!(lines.concat().replace(' ', ""), format!("short{word}end"), "nothing lost");
}

/// Code wider than the card (a module's path) goes on in rows, a long string broken after a
/// `/`: no row is wider than the room, nothing is lost, the colors stay the language's.
#[test]
fn a_long_line_of_code_goes_on_in_rows() {
    let mut ts = TextSystem::new();
    let palette = Palette::dark();
    let font = Font::new(palette.code_font.clone(), palette.code_font_size);
    let line = "module \"/Users/someone/work/site/node_modules/@tailwindcss/postcss/dist/index\"";
    let rows = wrap_code(line, "js", 200.0, &font, &mut ts, &palette);
    assert!(rows.len() > 2, "{}", rows.len());
    let text = |row: &CodeRow| row.iter().map(|(s, _)| *s).collect::<String>();
    for row in &rows {
        let width: f32 = row.iter().map(|(s, _)| ts.shape_line(s, &font).width).sum();
        assert!(width <= 200.0, "{:?} is {width}", text(row));
    }
    assert!(rows[..rows.len() - 1].iter().skip(1).all(|r| text(r).ends_with('/')));
    assert_eq!(rows.iter().map(text).collect::<String>(), line, "nothing lost");
    // A short one stays one row.
    assert_eq!(wrap_code("strlen($s)", "php", 200.0, &font, &mut ts, &palette).len(), 1);
}

/// A card taller than the room scrolls (the wheel), within its content; a short one
/// doesn't.
#[test]
fn a_long_card_scrolls_within_its_room() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut hover = CodeHover::default();
    hover.track(Some(0..4), Point::new(10.0, 10.0), true);
    let doc = (0..80).map(|i| format!("Line {i} of a long explanation.")).collect::<Vec<_>>();
    hover.show(Some(HoverCard {
        code: "phpinfo()".into(),
        doc: Some(doc.join("\n")),
        ..Default::default()
    }));
    let bounds = Rect::new(0.0, 0.0, 800.0, 400.0);
    hover.paint(bounds, 1.0, &mut ts, &mut Scene::default(), &palette);
    let card = hover.bounds().unwrap();
    assert!(card.max_y() <= bounds.max_y() && card.size.height < 400.0, "{card:?}");
    assert!(hover.scroll_by(200.0), "it scrolls");
    assert!(!hover.scroll_by(-1000.0) || hover.shown.as_ref().unwrap().scroll == 0.0);
    assert!(hover.scroll_by(1e6) && !hover.scroll_by(10.0), "not past its end");
    // A short one: nothing to scroll.
    hover.track(Some(10..14), Point::new(10.0, 10.0), true);
    hover.show(Some(HoverCard { code: "strlen()".into(), ..Default::default() }));
    hover.paint(bounds, 1.0, &mut ts, &mut Scene::default(), &palette);
    assert!(!hover.scroll_by(50.0));
}
