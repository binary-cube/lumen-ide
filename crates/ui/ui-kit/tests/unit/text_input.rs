use super::*;

#[test]
fn insert_replaces_selection_and_strips_newlines() {
    let mut t = TextInput::new("hello");
    t.select_all();
    t.insert("a\nb");
    assert_eq!(t.text(), "ab");
    assert_eq!(t.cursor(), 2);
}

#[test]
fn editing_with_unicode() {
    let mut t = TextInput::new("ăș");
    t.backspace(false);
    assert_eq!(t.text(), "ă");
    t.move_left(false, false);
    t.delete();
    assert_eq!(t.text(), "");
}

#[test]
fn word_navigation_and_selection() {
    let mut t = TextInput::new("foo bar_baz");
    t.move_left(true, true);
    assert_eq!(t.selected_text(), "bar_baz");
    t.move_left(false, false);
    assert_eq!(t.cursor(), 4);
    t.move_left(true, false);
    assert_eq!(t.cursor(), 0);
    t.move_right(true, false);
    assert_eq!(t.cursor(), 3);
    t.end(false);
    t.backspace(true);
    assert_eq!(t.text(), "foo ");
}

#[test]
fn undo_takes_a_typed_run_back_as_one_step() {
    let mut t = TextInput::new("");
    for c in ["a", "b", "c"] {
        t.insert(c);
    }
    t.insert(" ");
    t.select_all();
    t.insert("x");
    assert_eq!(t.text(), "x");
    assert!(t.undo());
    assert_eq!(t.text(), "abc ");
    assert_eq!(t.selected_text(), "abc ");
    assert!(t.undo());
    assert_eq!(t.text(), "");
    assert!(!t.undo());
    assert!(t.redo());
    assert_eq!(t.text(), "abc ");
    assert!(t.redo());
    assert_eq!(t.text(), "x");
    assert!(!t.redo());
    // A move in between starts a new step.
    t.insert("y");
    t.move_left(false, false);
    t.move_right(false, false);
    t.insert("z");
    assert!(t.undo());
    assert_eq!(t.text(), "xy");
}

#[test]
fn backspace_and_set_text_are_undone() {
    let mut t = TextInput::new("abc");
    t.backspace(false);
    t.set_text("other");
    assert!(t.undo());
    assert_eq!(t.text(), "ab");
    assert!(t.undo());
    assert_eq!(t.text(), "abc");
}

#[test]
fn a_secret_keeps_no_history() {
    let mut t = TextInput::new("");
    t.set_masked(true);
    t.insert("p");
    t.insert("w");
    assert!(!t.undo());
    assert_eq!(t.text(), "pw");
}

#[test]
fn three_clicks_select_everything() {
    let mut t = TextInput::new("foo bar");
    t.press(0.0, 3);
    assert_eq!(t.selected_text(), "foo bar");
}

/// The characters' places, found in one pass over the glyphs, are those each one's own
/// search finds: plain, accented, a secret's dots.
#[test]
fn the_characters_places_are_found_in_one_pass() {
    let style = TextInputStyle {
        font: Font::new(text_layout::FontFamily::SansSerif, 13.0),
        background: Rgba::TRANSPARENT,
        border: Rgba::TRANSPARENT,
        border_focused: Rgba::TRANSPARENT,
        text: Rgba::WHITE,
        placeholder: Rgba::WHITE,
        selection: Rgba::WHITE,
        caret: Rgba::WHITE,
        radius: 0.0,
        border_width: 0.0,
        padding: 0.0,
        caret_width: 1.0,
        lead: 0.0,
    };
    let mut ts = TextSystem::new();
    for masked in [false, true] {
        let mut input = TextInput::new("ăbc déf — ghi");
        input.masked = masked;
        let bounds = Rect::new(0.0, 0.0, 400.0, 20.0);
        input.paint(
            render_api::Scene::default().push_layer(None),
            &mut ts,
            bounds,
            false,
            "",
            &style,
        );
        let shown: String = if masked {
            std::iter::repeat_n('•', input.text().chars().count()).collect()
        } else {
            input.text().to_owned()
        };
        let shaped = ts.shape_line(&shown, &style.font);
        let painted = input.painted.borrow();
        let x0 = painted.stops[0].1 - shaped.x_at(0);
        for (n, (i, x)) in painted.stops.iter().enumerate() {
            let at = if masked { n * '•'.len_utf8() } else { *i };
            assert!((x - (x0 + shaped.x_at(at))).abs() < 0.01, "{masked} {i}");
        }
    }
}

/// A long text pasted into a narrow field, all of it selected: the selection is drawn
/// within the field, as its text is (scrolled to the caret, it starts far to the left).
#[test]
fn the_selection_stays_within_the_field() {
    let selection = Rgba::new(0.1, 0.2, 0.9, 1.0);
    let style = TextInputStyle {
        font: Font::new(text_layout::FontFamily::SansSerif, 13.0),
        background: Rgba::TRANSPARENT,
        border: Rgba::TRANSPARENT,
        border_focused: Rgba::TRANSPARENT,
        text: Rgba::WHITE,
        placeholder: Rgba::WHITE,
        selection,
        caret: Rgba::WHITE,
        radius: 0.0,
        border_width: 0.0,
        padding: 8.0,
        caret_width: 1.0,
        lead: 0.0,
    };
    let mut ts = TextSystem::new();
    let mut input = TextInput::new(&"postgresql://admin:admin@10.10.10.10/db ".repeat(8));
    input.select_all();
    let bounds = Rect::new(300.0, 0.0, 120.0, 24.0);
    let mut scene = render_api::Scene::default();
    input.paint(scene.push_layer(None), &mut ts, bounds, true, "", &style);
    let quads: Vec<Rect> = scene
        .layers()
        .iter()
        .flat_map(|l| l.quads.iter())
        .filter(|q| q.color == selection)
        .map(|q| q.bounds)
        .collect();
    assert_eq!(quads.len(), 1);
    assert!(quads[0].min_x() >= bounds.min_x() && quads[0].max_x() <= bounds.max_x(), "{quads:?}");
}
