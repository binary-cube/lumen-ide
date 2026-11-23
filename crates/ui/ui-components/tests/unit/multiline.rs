use super::*;

fn laid_out(text_value: &str, width: f32) -> (MultilineField, TextSystem) {
    let mut field = MultilineField::new(text_value);
    let mut text = TextSystem::new();
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let font = ds.ui_font(0.0);
    field.layout(&mut text, &font, width);
    (field, text)
}

#[test]
fn typing_lines_and_the_history() {
    let mut f = MultilineField::new("");
    f.insert("Fix");
    f.insert(" the cart");
    f.newline();
    f.insert("body");
    assert_eq!(f.text(), "Fix the cart\nbody");
    f.select_all();
    assert_eq!(f.selected_text(), "Fix the cart\nbody", "⌘A: every line");
    f.insert("x");
    assert_eq!(f.text(), "x", "typing replaces the selection");
    assert!(f.undo());
    assert_eq!(f.text(), "Fix the cart\nbody");
    assert!(f.redo());
    assert_eq!(f.text(), "x");
    f.set_text("one two three");
    f.left(true, true);
    assert_eq!(f.selected_text(), "three");
    f.backspace(false);
    f.backspace(true);
    assert_eq!(f.text(), "one ");
}

/// Wrapped at words to the width; a word longer than a line is cut; ↑ ↓ go by visual
/// lines, Home / End to theirs.
#[test]
fn the_text_wraps_and_the_caret_walks_its_lines() {
    let (mut f, _text) = laid_out("alpha beta gamma delta\nshort", 90.0);
    assert!(f.lines.len() >= 3, "{:?}", f.lines);
    assert!(f.lines.iter().all(|r| !f.text[r.clone()].ends_with('\n')));
    let joined: String = f.lines.iter().map(|r| &f.text[r.clone()]).collect();
    assert_eq!(joined.replace(' ', ""), "alphabetagammadeltashort".to_owned());
    f.move_to(0, false);
    f.vertical(true, false);
    assert_eq!(f.line_of(f.cursor()), 1);
    f.end(false, false);
    assert_eq!(f.cursor(), f.lines[1].end);
    f.vertical(true, true);
    assert!(!f.selected_text().is_empty(), "⇧↓ selects");
    let (long, _t) = laid_out("abcdefghijklmnopqrstuvwxyz", 40.0);
    assert!(long.lines.len() > 1, "a long word cut");
    assert_eq!(long.lines.last().unwrap().end, 26);
}

/// Its height grows from its least lines to its most, then it scrolls.
#[test]
fn it_grows_then_scrolls() {
    let mut text = TextSystem::new();
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut f = MultilineField::new("one");
    let three = f.height(&mut text, &ds, 400.0, (3, 10));
    f.set_text(&"line\n".repeat(30));
    let most = f.height(&mut text, &ds, 400.0, (3, 10));
    assert_eq!(most - three, 7.0 * 20.0);
}

/// To a screen reader: a text area named by its placeholder, its text and caret; its lines
/// aren't read again as loose texts.
#[test]
fn a_multiline_field_tells_its_text() {
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut field = MultilineField::new("one\ntwo");
    let mut text = TextSystem::new();
    let recording = ui_kit::access::begin();
    let spec = MultilineSpec { placeholder: "Message", focused: true, subject_limit: None };
    field.paint(
        &mut render_api::Scene::default(),
        &mut text,
        &ds,
        kernel::Rect::new(0.0, 0.0, 300.0, 120.0),
        spec,
    );
    let frame = recording.take();
    let area =
        frame.nodes().iter().find(|n| n.role == ui_kit::access::Role::TextArea).expect("the field");
    assert_eq!((&*area.name, area.value.as_deref()), ("Message", Some("one\ntwo")));
    assert!(area.lines.is_some() && area.state.focused);
    assert!(
        !frame.nodes().iter().any(|n| n.role == ui_kit::access::Role::Label && &*n.name == "one")
    );
}

/// Hovered: its scrollbar (when the text is longer than the field, shown once scrolled)
/// widens under the mouse and narrows when it leaves; the cursor is the text's over all of
/// it.
#[test]
fn its_bar_lights_under_the_mouse() {
    use crate::control::Control;
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let long: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
    let mut field = MultilineField::new(&long.join("\n"));
    let rect = Rect::new(0.0, 0.0, 300.0, 120.0);
    let mut scene = Scene::default();
    field.paint(&mut scene, &mut text, &ds, rect, MultilineSpec::new("Message"));
    assert!(field.scroll_by(40.0), "longer than the field: it scrolls");
    // Scrolled, its bar (hidden at rest) shows from the next frame.
    field.paint(&mut scene, &mut text, &ds, rect, MultilineSpec::new("Message"));
    let on_bar = Point::new(rect.max_x() - 4.0, 40.0);
    assert!(field.hover(on_bar), "the bar widens");
    assert!(!field.hover(on_bar), "still on it: nothing new");
    assert!(field.hover(Point::new(40.0, 40.0)), "left: it narrows");
    assert_eq!(Control::cursor(&field, Point::new(40.0, 40.0)), Some(crate::ViewerCursor::Text));
}

/// Drawn: a press in its text puts the caret there (a double press: the word), ⇧ extends.
#[test]
fn a_press_places_the_caret() {
    let palette = crate::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut field = MultilineField::new("one two three");
    let rect = Rect::new(0.0, 0.0, 400.0, 120.0);
    field.paint(&mut Scene::default(), &mut text, &ds, rect, MultilineSpec::new("Message"));
    let start = Point::new(field.area.min_x() + 1.0, field.area.min_y() + 4.0);
    assert!(field.press(start, 1, false));
    assert_eq!(field.selected_text(), "");
    let end = Point::new(field.area.max_x() - 2.0, field.area.min_y() + 4.0);
    assert!(field.press(end, 1, true));
    assert_eq!(field.selected_text(), "one two three", "⇧: to there");
    assert!(field.press(start, 2, false));
    assert_eq!(field.selected_text(), "one", "a double press: the word");
}
