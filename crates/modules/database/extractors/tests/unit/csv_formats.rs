use super::*;

#[test]
fn formats_are_added_changed_and_removed() {
    let mut e = FormatsEditor::new(Vec::new());
    assert_eq!(e.formats, CsvFormat::defaults());
    e.select(3);
    e.add();
    assert_eq!(e.selected, e.formats.len() - 1);
    let added = e.current().clone();
    assert!(added.id.starts_with("csv-") && added.id.len() > 20);
    assert_eq!(added.separator, "|", "made like the one shown");
    e.add();
    assert_ne!(e.current().name, added.name, "a name of its own");
    e.set_name("Hash");
    e.set_separator("#");
    e.set_quote(None);
    e.set_header(false);
    e.set_null("\\N");
    e.set_line_end(LineEnd::CrLf);
    assert_eq!(e.preview(), "1#Ana#a, \"b\"␍\n2#Ion#\\N");
    assert_eq!(e.problem(), None);
    let n = e.formats.len();
    assert!(e.remove());
    assert_eq!(e.formats.len(), n - 1);
    e.reset();
    assert_eq!(e.formats, CsvFormat::defaults());
    // Never the last one.
    let mut one = FormatsEditor::new(vec![CsvFormat::new("x", "X", ",", true)]);
    assert!(!one.remove());
}

#[test]
fn what_can_t_be_saved_says_why() {
    let mut e = FormatsEditor::new(Vec::new());
    e.set_name(" ");
    assert!(e.problem().is_some());
    e.set_name("CSV");
    e.set_separator("");
    assert!(e.problem().is_some());
    e.set_separator("\"");
    assert!(e.problem().is_some(), "the quote inside the separator");
    e.set_quote(Some('\''));
    assert_eq!(e.problem(), None);
    e.select(1);
    e.set_name("CSV");
    assert!(e.problem().is_some(), "two of the same name");
}

#[test]
fn the_view_gives_the_formats_edited() {
    let mut view = FormatsView::default();
    view.set_data(&FormatsRequest { id: 1, formats: CsvFormat::defaults() });
    assert!(view.command("add"));
    assert!(view.wants_text(), "the new format's name has the keys");
    assert!(view.text_input("Mine"));
    assert_eq!(view.editor().unwrap().current().name, "Mine");
    assert!(view.edit_key(ViewerKey::Escape), "the field lets go first");
    assert!(!view.wants_text());
    assert!(view.command("select:0"));
    assert_eq!(view.editor().unwrap().selected, 0);
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    assert!(matches!(view.take_action(), Some(ViewerAction::RunCommand(ref id)) if id == GIVEN.0));
    let given = GIVEN_FORMATS.lock().unwrap().take().unwrap();
    assert_eq!(given.len(), CsvFormat::defaults().len() + 1);
    assert_eq!(given.last().unwrap().name, "Mine");
}

#[test]
fn esc_closes_without_giving_anything() {
    let mut view = FormatsView::default();
    view.set_data(&FormatsRequest { id: 2, formats: CsvFormat::defaults() });
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(matches!(view.take_action(), Some(ViewerAction::Close)));
    assert!(!view.closed());
}

/// The fields are the modal's controls: a press on one gives it the keys (typed through the
/// controls), a press on another writes what the first held
/// and moves the keys there, a press beside them takes the keys away; Tab stays in a field.
#[test]
fn a_press_moves_the_keys_between_the_fields() {
    let mut view = FormatsView::default();
    view.set_data(&FormatsRequest { id: 3, formats: CsvFormat::defaults() });
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1400.0, HEIGHT * 2.0),
        scale: 2.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let [name, separator, _] = view.fields;
    let on = |r: Rect| Point::new(r.max_x() - 6.0, r.min_y() + r.size.height / 2.0);
    view.hover(on(separator));
    assert_eq!(view.cursor_at(on(separator)), Some(ViewerCursor::Text));
    view.click(on(separator));
    view.release();
    assert_eq!(view.field(), Some(Field::Separator));
    assert!(view.text_input("~"));
    assert_eq!(view.editor().unwrap().current().separator, "~");
    assert!(!view.edit_key(ViewerKey::Tab), "Tab moves nothing");
    assert_eq!(view.field(), Some(Field::Separator));
    view.click(on(name));
    view.release();
    assert_eq!(view.field(), Some(Field::Name));
    assert!(view.text_input("x"));
    assert!(view.editor().unwrap().current().name.ends_with('x'));
    view.click(Point::new(2.0, 2.0));
    assert!(!view.wants_text());
}
