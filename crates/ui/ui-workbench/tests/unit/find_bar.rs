use super::*;
use icons::Icons;
use render_api::Scene;

/// The bar painted in a strip 800 wide, `error` saying the pattern is bad.
fn painted(bar: &mut FindBar, text: &mut TextSystem, error: bool) -> Scene {
    let (palette, icons) = (Palette::dark(), Icons::builtin());
    let options = SearchQuery::default();
    let spec = FindBarSpec {
        bounds: Rect::new(0.0, 0.0, 800.0, bar.height(1.0)),
        options: &options,
        counter: (0, 0),
        error,
        focused: true,
        scale: 1.0,
    };
    let mut scene = Scene::default();
    bar.paint(spec, Painter { text, scene: &mut scene, palette: &palette, icons: &icons });
    scene
}

/// The middle of the first button registered as `hit`.
fn button(bar: &FindBar, hit: FindHit) -> Point {
    let (r, _) = bar.hits.iter().find(|(_, h)| *h == hit).copied().expect("the button");
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

/// The options sit inside the search field and answer before it; a press on the text puts
/// the caret and holds the pointer, a drag selects.
#[test]
fn the_options_answer_before_the_field_and_the_field_selects_by_drag() {
    let mut text = TextSystem::new();
    let mut bar = FindBar::default();
    bar.query.set_text("hello world");
    painted(&mut bar, &mut text, false);
    let area = bar.query.area();
    for hit in [FindHit::ToggleCase, FindHit::ToggleWord, FindHit::ToggleRegex] {
        let at = button(&bar, hit);
        assert!(area.contains(at), "{hit:?} inside the field");
        assert_eq!(bar.press(at, 1), Some(hit));
        assert!(!bar.held());
    }
    let start = Point::new(area.min_x() + 40.0, area.min_y() + area.size.height / 2.0);
    assert_eq!(bar.press(start, 1), Some(FindHit::Field(FindField::Query)));
    assert!(bar.held(), "the field holds the pointer");
    assert!(bar.drag(Point::new(start.x + 200.0, start.y)));
    bar.release();
    assert!(!bar.held());
    assert!(!bar.query.input.selection().is_empty(), "the drag selected");
}

/// × empties the search field and says so.
#[test]
fn the_clear_button_empties_the_query() {
    let mut text = TextSystem::new();
    let mut bar = FindBar::default();
    bar.query.set_text("needle");
    painted(&mut bar, &mut text, false);
    let clear = bar.query.clear_area().expect("× with text");
    let at = Point::new(clear.min_x() + clear.size.width / 2.0, clear.min_y() + 2.0);
    assert_eq!(bar.hit(at), Some(FindHit::Clear));
    assert_eq!(bar.press(at, 1), Some(FindHit::Clear));
    assert_eq!(bar.query.text(), "");
}

/// The focus is the bar's one: Replace only while its row shows; hidden, the keys go to
/// the search field.
#[test]
fn the_focus_goes_between_the_fields() {
    let mut text = TextSystem::new();
    let mut bar = FindBar::default();
    assert_eq!(bar.focus(), FindField::Query);
    bar.show_replace = true;
    painted(&mut bar, &mut text, false);
    bar.set_focus(FindField::Replace);
    assert_eq!(bar.focus(), FindField::Replace);
    bar.focused_input().insert("with");
    assert_eq!(bar.replace.text(), "with");
    // A press on the replacement's field gives it the focus too.
    bar.set_focus(FindField::Query);
    // Its row is under the search field's, the field the same width.
    let q = bar.query.area();
    let r = Rect::new(q.min_x(), q.min_y() + ROW, q.size.width, q.size.height);
    let at = Point::new(r.min_x() + 50.0, r.min_y() + r.size.height / 2.0);
    assert_eq!(bar.press(at, 1), Some(FindHit::Field(FindField::Replace)));
    assert_eq!(bar.focus(), FindField::Replace);
    bar.release();
    bar.show_replace = false;
    assert_eq!(bar.focus(), FindField::Query);
    bar.focused_input().insert("q");
    assert_eq!(bar.query.text(), "q");
}

/// A long query never runs into the options; a bad pattern turns the field's border red.
#[test]
fn a_long_query_stays_clear_of_the_options_and_a_bad_one_is_red() {
    let mut text = TextSystem::new();
    let mut bar = FindBar::default();
    bar.set_focus(FindField::Query);
    bar.query.input.insert(&"a very long search pattern ".repeat(20));
    let palette = Palette::dark();
    let scene = painted(&mut bar, &mut text, true);
    let first_option = button(&bar, FindHit::ToggleCase).x - TOGGLE_W / 2.0;
    let area = bar.query.area();
    let font = Font::new(palette.ui_font.clone(), palette.ui_font_size - 0.5);
    let widest = text.shape_line("W", &font).width;
    let letters = scene.layers().iter().flat_map(|l| l.glyphs.iter()).filter(|g| {
        !icons::is_icon(g.key) && g.origin.x > area.min_x() + 30.0 && g.origin.x < first_option
    });
    for g in letters {
        assert!(g.origin.x + widest * 0.5 <= first_option, "a letter under the options");
    }
    let red = scene
        .layers()
        .iter()
        .flat_map(|l| l.quads.iter())
        .any(|q| q.bounds == area && q.border_color == palette.danger);
    assert!(red, "the border in the danger color");
}
