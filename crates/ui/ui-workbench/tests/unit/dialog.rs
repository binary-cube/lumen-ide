use super::*;

fn spec() -> DialogSpec {
    DialogSpec {
        tone: Tone::Accent,
        icon: "file-pen",
        title: "Unsaved changes".into(),
        message: "Save the changes to index.php?".into(),
        buttons: vec![
            DialogButton::new("cancel", "Cancel", ButtonKind::Secondary),
            DialogButton::new("discard", "Don't Save", ButtonKind::Destructive),
            DialogButton::new("save", "Save", ButtonKind::Primary),
        ],
        cancel: "cancel",
        input: None,
        check: None,
        list: Vec::new(),
    }
}

#[test]
fn keys_buttons_and_outside_click_ignored() {
    let mut dialog = Dialog::default();
    dialog.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    dialog.open(spec());
    assert_eq!(dialog.confirm(), Some("save"), "focus pe butonul principal");
    assert_eq!(dialog.cancel(), Some("cancel"));
    dialog.move_focus(1);
    assert_eq!(dialog.confirm(), Some("cancel"), "the focus goes round");
    dialog.paint(
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let save = dialog.rects[2];
    assert_eq!(dialog.hit(Point::new(save.min_x() + 4.0, save.min_y() + 4.0)), Some("save"));
    assert_eq!(dialog.hit(Point::new(2.0, 2.0)), None, "a click outside doesn't close it");
    dialog.close();
    assert!(!dialog.is_open());
}

#[test]
fn the_checkbox_turns_the_dialog_to_danger() {
    let mut dialog = Dialog::default();
    dialog.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    let mut with_check = spec();
    with_check.check = Some(DialogCheck {
        label: "Also delete".into(),
        warning: "Gone to the trash".into(),
        checked_label: "Delete from disk".into(),
    });
    dialog.open(with_check);
    assert!(!dialog.checked(), "unchecked on open");
    let paint = |dialog: &mut Dialog| {
        dialog.paint(
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
    };
    paint(&mut dialog);
    let check = dialog.check_rect.unwrap_or_default();
    assert!(dialog.check_at(Point::new(check.min_x() + 6.0, check.min_y() + 6.0)));
    assert!(dialog.checked());
    let shown = dialog.shown().unwrap_or_else(spec);
    assert_eq!(shown.tone, Tone::Danger);
    assert_eq!(shown.buttons[2].label, "Delete from disk");
    paint(&mut dialog);
    assert!(dialog.toggle_check());
    assert!(!dialog.checked());
    // Reopened: unchecked again; without a checkbox nothing toggles.
    dialog.open(spec());
    assert!(!dialog.toggle_check());
}

/// A new line in the message starts a new row (two: an empty one between), the highlights
/// kept across it.
#[test]
fn a_message_s_lines_start_new_rows() {
    // The modals' font (as the dialog's own: the design system's).
    let palette = Palette::dark();
    let font = Ctx::new(&palette, 1.0).ui_font(0.0);
    let rows = wrap_message(
        "Delete 2 files?\n\n• **a.php**\n• **b.php**",
        (&font, &font),
        1000.0,
        &mut TextSystem::new(),
    );
    let text: Vec<String> =
        rows.iter().map(|r| r.iter().map(|(w, _)| w.as_str()).collect()).collect();
    assert_eq!(text, ["Delete 2 files?", "", "• a.php", "• b.php"]);
    assert_eq!(rows[2], [("• ".to_owned(), false), ("a.php".to_owned(), true)]);
}

/// A path wider than the dialog breaks after its folders' slashes, never past the edge.
#[test]
fn a_long_path_in_the_message_breaks_after_its_slashes() {
    let palette = Palette::dark();
    let font = Ctx::new(&palette, 1.0).ui_font(0.0);
    let strong = font.clone().with_weight(text_layout::Font::SEMIBOLD);
    let mut text = TextSystem::new();
    let path = "admin/control/api/components/ai/analyze/image-studio/aaa/generate.php";
    let message = format!("**{path}** was deleted here; the server still has it.");
    let rows = wrap_message(&message, (&font, &strong), 260.0, &mut text);
    assert!(rows.len() >= 3, "{rows:?}");
    for row in &rows {
        let width: f32 = row
            .iter()
            .map(|(w, bold)| text.shape_line(w, if *bold { &strong } else { &font }).width)
            .sum();
        assert!(width <= 260.0 + 0.5, "{row:?} too wide");
    }
    let bold: String = rows.iter().flatten().filter(|(_, b)| *b).map(|(w, _)| w.as_str()).collect();
    assert_eq!(bold, path, "nothing lost");
    assert!(rows[0].iter().all(|(w, _)| w.ends_with('/')), "cut after a slash: {:?}", rows[0]);
}

#[test]
fn a_long_title_wraps_and_a_long_name_breaks() {
    let palette = Palette::dark();
    let font = Ctx::new(&palette, 1.0).ui_font(1.5);
    let mut text = TextSystem::new();
    let title = "Discard the changes in Implementare_identificare_completa_a_clientului_nou.txt?";
    let rows = wrap_title(title, &font, 260.0, &mut text);
    assert!(rows.len() >= 3, "{rows:?}");
    for row in &rows {
        assert!(text.shape_line(row, &font).width <= 260.0, "{row:?} too wide");
    }
    assert_eq!(rows.concat().replace(' ', ""), title.replace(' ', ""), "nothing lost");
    assert_eq!(wrap_title("Short?", &font, 260.0, &mut text), ["Short?"]);
}

/// A dialog listing `n` files deleted, under one folder, painted in `viewport`.
fn listing(n: usize, viewport: Rect) -> Dialog {
    let mut dialog = Dialog::default();
    dialog.set_viewport(viewport, 1.0);
    let mut spec = spec();
    spec.list =
        (0..n).map(|i| format!("admin/control/api/components/image-studio/f{i}.php")).collect();
    dialog.open(spec);
    dialog.paint(
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    dialog
}

/// The folder the entries share is shown once (ending in `/`, cut only at a `/`); none for
/// one entry, or entries sharing nothing.
#[test]
fn the_entries_common_folder_is_shown_once() {
    let list = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(common_folder(&list(&["a/b/c.php", "a/b/d/e.php"])), "a/b/");
    assert_eq!(common_folder(&list(&["a/bc/x", "a/bd/y"])), "a/");
    assert_eq!(common_folder(&list(&["a/b", "a/b/c"])), "a/");
    assert_eq!(common_folder(&list(&["x.php", "a/y.php"])), "");
    assert_eq!(common_folder(&list(&["a/b/c.php"])), "");
    assert_eq!(common_folder(&list(&["ă/ș/1", "ă/ș/2"])), "ă/ș/");
    assert_eq!(common_folder(&[]), "");
}

/// Many entries scroll in their box (the wheel, ↑ / ↓), within how far they go; a few
/// don't scroll at all.
#[test]
fn a_long_list_scrolls_within_its_box() {
    let window = Rect::new(0.0, 0.0, 1200.0, 800.0);
    let mut dialog = listing(33, window);
    let row = 24.0;
    let most = 33.0 * row + 8.0 - (LIST_ROWS as f32 * row + 8.0);
    assert!((dialog.list_max - most).abs() < 0.5, "{} vs {most}", dialog.list_max);
    assert!(!dialog.scroll(-10.0), "already at the top");
    assert!(dialog.scroll_rows(1.0));
    assert_eq!(dialog.list_scroll, row);
    assert!(dialog.scroll(1e6));
    assert_eq!(dialog.list_scroll, dialog.list_max, "no further than the last row");
    assert!(!dialog.scroll(10.0));
    let mut few = listing(3, window);
    assert_eq!(few.list_max, 0.0);
    assert!(!few.scroll(50.0), "three rows fit: nothing to scroll");
}

/// However many the entries and however low the window, the card fits it — the list
/// giving up rows — so its buttons stay in sight.
#[test]
fn the_card_with_a_list_fits_the_window() {
    for height in [800.0, 420.0, 300.0] {
        let window = Rect::new(0.0, 0.0, 1200.0, height);
        let dialog = listing(500, window);
        assert!(dialog.card.min_y() >= window.min_y(), "{height}: {:?}", dialog.card);
        assert!(dialog.card.max_y() <= window.max_y(), "{height}: {:?}", dialog.card);
        let buttons = dialog.rects.iter().all(|b| b.max_y() <= window.max_y());
        assert!(buttons, "{height}: the buttons in sight");
    }
    let tall = listing(500, Rect::new(0.0, 0.0, 1200.0, 800.0));
    let low = listing(500, Rect::new(0.0, 0.0, 1200.0, 300.0));
    assert!(low.card.size.height < tall.card.size.height, "fewer rows in a low window");
}

/// The list's section goes down to the footer, the card's whole width.
#[test]
fn the_list_goes_down_to_the_buttons() {
    let mut dialog = listing(33, Rect::new(0.0, 0.0, 1200.0, 800.0));
    let mut scene = Scene::default();
    dialog.paint(&Icons::builtin(), &mut TextSystem::new(), &mut scene, &Palette::dark());
    let footer = dialog.card.max_y() - 58.0;
    let bottom = dialog.list_bottom;
    assert!((footer - bottom).abs() < 1.5, "the list ends at {bottom}, the footer at {footer}");
}

/// Without a list, the card is as tall as before there were lists.
#[test]
fn without_a_list_the_card_is_as_before() {
    let mut dialog = Dialog::default();
    dialog.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    dialog.open(spec());
    dialog.paint(
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let with_list = listing(1, Rect::new(0.0, 0.0, 1200.0, 800.0));
    assert!(with_list.card.size.height > dialog.card.size.height);
    assert_eq!(dialog.list_max, 0.0);
}

/// The list's thumb dragged: the list follows it (to its end at most), until released.
#[test]
fn the_list_s_thumb_drags_it() {
    let mut dialog = listing(33, Rect::new(0.0, 0.0, 1200.0, 800.0));
    let top = dialog.list_bottom - (LIST_ROWS as f32 * 24.0 + 8.0);
    let x = dialog.card.max_x() - 3.0;
    assert!(!dialog.press(Point::new(dialog.card.min_x() + 30.0, top + 10.0)), "not on it");
    assert!(dialog.press(Point::new(x, top + 10.0)), "the thumb, at the top");
    assert!(dialog.held());
    assert_eq!(dialog.list_scroll, 0.0, "pressed on the thumb: it doesn't jump");
    assert!(dialog.drag(Point::new(x, top + 60.0)));
    let moved = dialog.list_scroll;
    assert!(moved > 0.0);
    dialog.drag(Point::new(x, top + 5000.0));
    assert_eq!(dialog.list_scroll, dialog.list_max, "no further than the last row");
    assert!(dialog.release());
    assert!(!dialog.held());
    assert!(!dialog.drag(Point::new(x, top)), "released: nothing follows");
}
