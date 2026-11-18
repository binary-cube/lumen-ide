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
