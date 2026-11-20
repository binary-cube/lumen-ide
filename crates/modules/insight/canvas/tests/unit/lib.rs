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
    assert_eq!(canvas.search.as_ref().map(|s| s.query.as_str()), Some("x"));
    assert!(canvas.edit_key(ViewerKey::Escape));
    assert!(canvas.search.is_none());
    assert!(!canvas.edit_key(ViewerKey::Escape) && !canvas.text_input("y"));
    assert!(!canvas.command("select_all"));
}
