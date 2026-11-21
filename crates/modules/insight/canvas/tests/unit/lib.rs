use super::*;

/// Its actions do what its keys did: zoom, the modes; the keys themselves are the
/// keymap's now (the canvas takes no character).
#[test]
fn the_actions_do_what_the_keys_did() {
    let mut canvas = Canvas::default();
    let zoom = canvas.zoom;
    assert!(canvas.command("zoom_in"));
    assert!(canvas.zoom > zoom);
    assert!(canvas.command("zoom_out"));
    assert!((canvas.zoom - zoom).abs() < 1e-4);
    assert!(canvas.command("files"));
    assert_eq!(canvas.mode, Mode::Files);
    assert!(canvas.command("code") && canvas.mode == Mode::Code);
    assert!(!canvas.key('1'), "a key alone does nothing: the keymap sends the action");
    let ids: Vec<&str> = ACTIONS.iter().map(|a| a.3).collect();
    assert_eq!(ids, ["zoom_in", "zoom_out", "fit", "functions", "files", "code"]);
}

/// A drag moves one edge: only its dots are made again, the others' kept as they were;
/// fewer edges drop the rest.
#[test]
fn only_a_moved_edge_s_dots_are_made_again() {
    let shape =
        |x: f32| [0f32.to_bits(), 0f32.to_bits(), x.to_bits(), 80f32.to_bits(), 2f32.to_bits(), 0];
    let mut kept = Vec::new();
    update_dots(&mut kept, &[shape(200.0), shape(300.0)]);
    let still = kept[1].1.1.as_ptr();
    let before = kept[0].1.1.len();
    update_dots(&mut kept, &[shape(400.0), shape(300.0)]);
    assert_eq!(kept[1].1.1.as_ptr(), still, "the unmoved edge is kept");
    assert!(kept[0].1.1.len() > before, "the moved one, longer, made again");
    update_dots(&mut kept, &[shape(400.0)]);
    assert_eq!(kept.len(), 1);
}

/// The search open: the keys are its own (Esc closes it); closed, the canvas takes none.
#[test]
fn the_open_search_has_the_keys() {
    let mut canvas = Canvas { search: Some(search::Search::default()), ..Canvas::default() };
    assert!(canvas.wants_text());
    assert!(canvas.text_input("x"));
    assert_eq!(canvas.search.as_ref().map(search::Search::query), Some("x"));
    assert!(canvas.command("select_all"), "⌘A: the search's");
    assert_eq!(canvas.selected_text().as_deref(), Some("x"));
    assert!(canvas.paste("ab\ncd"));
    assert_eq!(canvas.search.as_ref().map(search::Search::query), Some("ab cd"), "one line");
    assert!(canvas.edit_key(ViewerKey::Escape));
    assert!(canvas.search.is_none());
    assert!(!canvas.edit_key(ViewerKey::Escape) && !canvas.text_input("y"));
    assert!(!canvas.command("select_all"));
}

/// The search's field keeps the ⌘K key on its right, open or closed, with or without text:
/// × (once there's text) sits left of the key's room.
#[test]
fn the_search_field_keeps_its_key_with_the_clear_button_left_of_it() {
    let palette = ui_components::palette::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let (mut text, icons) = (TextSystem::new(), icons::Icons::builtin());
    let field = Rect::new(0.0, 0.0, 340.0, 40.0);
    let key = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" };
    let key_w = text.shape_line(key, &ds.ui_font(-2.0)).width;
    let mut canvas = Canvas::default();
    for typed in [None, Some(""), Some("user")] {
        canvas.search = typed.map(|q| {
            let mut search = search::Search::default();
            search.field.set_text(q);
            search
        });
        let mut layer = render_api::Layer::default();
        canvas.paint_search_field(
            Pen { layer: &mut layer, text: &mut text, ds: &ds },
            &icons,
            field,
        );
        let clear = canvas.search.as_ref().and_then(|s| s.field.clear_area());
        assert_eq!(clear.is_some(), typed == Some("user"), "× only with text");
        if let Some(clear) = clear {
            assert!(clear.max_x() <= field.max_x() - key_w, "× left of the key");
        }
    }
}
