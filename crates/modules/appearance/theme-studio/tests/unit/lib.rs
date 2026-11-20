use super::*;
use rows::CaretRow;
use std::sync::Arc;
use theming::decorations::{
    CornerStyle, DecorationsSpec, FieldValue, LayerKind, ParticleShape, decode, presets,
};
use theming::options::{Label, OptionChoice, OptionKind, OptionValue, ThemeOption};
use theming::studio::{StudioTheme, layer_color_key};

fn studio_with(base: (&str, &str)) -> Studio {
    studio(base)
}

fn studio(base: (&str, &str)) -> Studio {
    let themes = vec![StudioTheme::plain(base.0, base.1, "")];
    Studio { input: Some(StudioInput { themes, current: base.0.to_owned() }), ..Studio::default() }
}

/// A theme made here, saved again under its name, keeps its id; anything else gets a new
/// UUID (two themes named alike never overwrite each other).
#[test]
fn saved_themes_get_their_own_identity() {
    let own = studio(("custom-6f1c0a52", "Mine"));
    assert_eq!(own.saved_id("Mine"), "custom-6f1c0a52");
    let renamed = own.saved_id("Other");
    assert!(renamed.starts_with("custom-") && renamed != "custom-6f1c0a52");
    let from_builtin = studio(("default-dark", "Mine"));
    let (a, b) = (from_builtin.saved_id("Mine"), from_builtin.saved_id("Mine"));
    assert_ne!(a, b, "a new identity each time");
    let uuid = a.strip_prefix("custom-").unwrap();
    assert_eq!(uuid::Uuid::parse_str(uuid).map(|u| u.get_version_num()), Ok(4));
}

const CHRISTMAS: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../themes/christmas-dark.toml"));
const CHRISTMAS_LIGHT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../themes/christmas-light.toml"));
const PLAIN: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../themes/default-dark.toml"));
const LIGHT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../themes/default-light.toml"));

/// The Christmas theme's own settings (as the app reads them).
fn christmas_options() -> Vec<ThemeOption> {
    let choice =
        |id: &str| OptionChoice { id: id.into(), label: Label::plain(id), palette: Vec::new() };
    vec![
        ThemeOption {
            id: "lights".into(),
            label: Label::plain("Lights"),
            kind: OptionKind::Choice(vec![choice("warm"), choice("colorful"), choice("icy")]),
            default: OptionValue::Choice("warm".into()),
        },
        ThemeOption {
            id: "snow".into(),
            label: Label::plain("Snow"),
            kind: OptionKind::Slider { min: 0.0, max: 0.8, step: 0.1 },
            default: OptionValue::Number(0.5),
        },
        ThemeOption {
            id: "tint".into(),
            label: Label::plain("Tint"),
            kind: OptionKind::Color { palette: Some("accent".into()) },
            default: OptionValue::Color(Rgba::WHITE),
        },
        ThemeOption {
            id: "bows".into(),
            label: Label::plain("Bows"),
            kind: OptionKind::Toggle,
            default: OptionValue::Bool(true),
        },
    ]
}

fn input() -> StudioInput {
    StudioInput {
        themes: vec![
            StudioTheme {
                decorations: DecorationsSpec::of(presets::layers("christmas", true).unwrap()),
                options: christmas_options(),
                ..StudioTheme::plain("christmas-dark", "Christmas Dark", CHRISTMAS)
            },
            StudioTheme::plain("default-dark", "Dark", PLAIN),
            StudioTheme::plain("default-light", "Light", LIGHT),
            StudioTheme::plain("christmas-light", "Christmas Light", CHRISTMAS_LIGHT),
        ],
        current: "christmas-dark".into(),
    }
}

/// The studio on four themes (Christmas first, with its decorations and settings), drawn
/// once at 1400 × 6000 (each category's whole panel shown).
fn opened() -> (Studio, TextSystem, Scene) {
    let mut studio = Studio::default();
    studio.set_data(&input());
    let (mut text, mut scene) = (TextSystem::new(), Scene::default());
    paint(&mut studio, &mut text, &mut scene);
    (studio, text, scene)
}

fn paint_at(studio: &mut Studio, text: &mut TextSystem, scene: &mut Scene, w: f32, h: f32) {
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    scene.reset(palette.editor_background);
    studio.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, w, h),
        scale: 1.0,
        text,
        scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

fn paint(studio: &mut Studio, text: &mut TextSystem, scene: &mut Scene) {
    paint_at(studio, text, scene, 1400.0, 6000.0);
}

/// Category `c` shown and drawn.
fn show(studio: &mut Studio, text: &mut TextSystem, scene: &mut Scene, c: Category) {
    assert!(studio.show(c), "{c:?}");
    paint(studio, text, scene);
}

/// The theme the studio previews now (its text).
fn previewed(studio: &mut Studio) -> String {
    match studio.take_action() {
        Some(ViewerAction::Request(AppRequest::PreviewTheme(src))) => src,
        other => panic!("no preview: {other:?}"),
    }
}

fn click_on(studio: &mut Studio, hit: Hit) -> Option<ViewerAction> {
    let r = studio.hits.iter().find(|(_, h)| *h == hit).map(|(r, _)| *r);
    let r = r.unwrap_or_else(|| panic!("no {hit:?}"));
    studio.click(Point::new(r.min_x() + 4.0, r.min_y() + r.size.height / 2.0))
}

fn preview_of(action: Option<ViewerAction>) -> String {
    match action {
        Some(ViewerAction::Request(AppRequest::PreviewTheme(src))) => src,
        other => panic!("{other:?}"),
    }
}

fn has(studio: &Studio, hit: &Hit) -> bool {
    studio.hits.iter().any(|(_, h)| h == hit)
}

/// Every setting the studio had before has its place: each category drawn shows a row for
/// every one of its settings (every color, every caret row, every effect, every theme
/// setting, the name, the themes to start from), and the footer its scope and buttons.
#[test]
fn every_setting_has_its_place() {
    let (mut studio, mut text, mut scene) = opened();
    let draft = studio.draft.clone().unwrap();
    let entries = model::catalog(&draft, studio.input.as_ref().unwrap(), false);
    // Every color the studio edited (and the states the theme has), every caret row.
    for key in theming::studio::UI_TOKENS {
        assert!(entries.iter().any(|e| e.id == SettingId::Token(key)), "{key}");
    }
    for key in theming::studio::SYNTAX_TOKENS {
        let key = format!("syntax.{key}");
        assert!(
            entries
                .iter()
                .any(|e| e.id == SettingId::Token(Box::leak(key.clone().into_boxed_str()))),
            "{key}"
        );
    }
    for row in CaretRow::ALL {
        assert!(entries.iter().any(|e| e.id == SettingId::Caret(row)), "{row:?}");
    }
    assert!(entries.iter().any(|e| e.id == SettingId::Token("red")), "a state the theme has");
    for c in Category::shown(&draft).collect::<Vec<_>>() {
        show(&mut studio, &mut text, &mut scene, c);
        for e in entries.iter().filter(|e| e.category == c) {
            assert!(studio.rows_at.iter().any(|(id, _)| *id == e.id), "{c:?}: {:?}", e.id);
        }
        // The categories, the footer.
        for other in Category::shown(&draft) {
            assert!(has(&studio, &Hit::Category(other)));
        }
        for hit in [Hit::Scope(false), Hit::Scope(true), Hit::Save, Hit::Discard, Hit::Search] {
            assert!(has(&studio, &hit), "{hit:?}");
        }
    }
    // Every theme to start from but the preview one, and the name's field.
    show(&mut studio, &mut text, &mut scene, Category::General);
    for i in 0..4 {
        assert!(has(&studio, &Hit::Base(i)), "{i}");
    }
    assert!(has(&studio, &Hit::Name));
}

/// The categories: Options only for a theme with settings of its own; each with its own
/// sections, in the same rhythm (the first right under the title, twice the space between
/// any two).
#[test]
fn each_category_has_its_sections_in_one_rhythm() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Interface);
    let ids: Vec<&str> = studio.headers.iter().map(|(id, ..)| *id).collect();
    assert_eq!(ids, ["backgrounds", "text", "accent", "states", "borders", "backdrop", "floating"]);
    let (_, first, above) = studio.headers[0];
    assert_eq!(above, 0.0);
    assert!(first.min_y() > studio.list.min_y() + 40.0, "under the title");
    for (id, _, above) in &studio.headers[1..] {
        assert_eq!(*above, 2.0 * ds::SECTION_SPACE, "{id}");
    }
    show(&mut studio, &mut text, &mut scene, Category::Caret);
    let ids: Vec<&str> = studio.headers.iter().map(|(id, ..)| *id).collect();
    assert_eq!(ids, ["shape", "animation"]);
    // A theme without settings of its own: no such category.
    studio.load_base(1);
    paint(&mut studio, &mut text, &mut scene);
    assert!(!has(&studio, &Hit::Category(Category::Options)));
    assert!(!studio.show(Category::Options));
}

/// The effects: a card each (its header: open / close, its handle, its switch, its menu),
/// every option of its kind once opened (choices as pills or segments, amounts as sliders,
/// colors as swatches); the gallery: every preset and every kind.
#[test]
fn every_layer_shows_every_option_of_its_kind() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Effects);
    let draft = studio.draft.clone().unwrap();
    assert_eq!(draft.decorations.layers.len(), 3);
    for i in 0..3 {
        for hit in [Hit::Fold(i), Hit::Handle(i), Hit::Toggle(i), Hit::Menu(i)] {
            assert!(has(&studio, &hit), "{hit:?}");
        }
    }
    assert!(studio.sliders.is_empty(), "folded: one line each");
    assert!(studio.command("open-all"));
    paint(&mut studio, &mut text, &mut scene);
    for (i, layer) in draft.decorations.layers.iter().enumerate() {
        for (n, field) in layer.spec.kind().fields().iter().enumerate() {
            let row = Row::Field { layer: i, field: n };
            match field.kind {
                theming::decorations::FieldKind::Choice(ids) => {
                    for c in 0..ids.len() {
                        assert!(has(&studio, &Hit::Choice(row, c)), "{i} {}", field.key);
                    }
                }
                theming::decorations::FieldKind::Amount(_) => {
                    assert!(studio.sliders.iter().any(|(_, r)| *r == row), "{i} {}", field.key);
                }
                theming::decorations::FieldKind::Color
                | theming::decorations::FieldKind::Colors { .. } => {
                    let key = layer_color_key(i, field.key, 0);
                    let empty = matches!(layer.spec.get(field.key), Some(FieldValue::Colors(l)) if l.is_empty());
                    assert!(empty || has(&studio, &Hit::Token(key.clone())), "{key}");
                }
                theming::decorations::FieldKind::Text => {}
            }
        }
    }
    assert!(has(&studio, &Hit::Overlay));
    // The gallery: every preset, every kind.
    let _ = click_on(&mut studio, Hit::Gallery(true));
    paint(&mut studio, &mut text, &mut scene);
    for i in 1..=presets::ALL.len() {
        assert!(has(&studio, &Hit::Choice(Row::Preset, i)), "preset {i}");
    }
    assert_eq!(Row::Preset.current(&draft), 1, "the Christmas preset's layers");
    for k in 0..LayerKind::ALL.len() {
        assert!(has(&studio, &Hit::Choice(Row::Add, k)), "kind {k}");
    }
    // While it's open, nothing behind it takes a click (the effects' panel is under it);
    // × or Esc closes it.
    let fold = studio.hits.iter().find(|(_, h)| *h == Hit::Fold(0)).map(|(r, _)| *r).unwrap();
    let below = Point::new(fold.max_x() - 2.0, studio.bounds.max_y() - 2.0);
    assert!(studio.click(below).is_none());
    assert!(studio.gallery_open && has(&studio, &Hit::Gallery(false)));
    assert!(studio.edit_key(ViewerKey::Escape));
    assert!(!studio.gallery_open);
}

/// Edited live: a choice clicked, a slider dragged, a layer added (from the gallery) and
/// removed (from its menu), a color picked, one color more and one fewer, a preset — each
/// previewed at once.
#[test]
fn layers_are_edited_and_previewed_live() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Effects);
    studio.command("open-all");
    paint(&mut studio, &mut text, &mut scene);
    let _ = studio.take_action();
    // The corners (the second layer): holly.
    let style = Row::Field { layer: 1, field: 0 };
    let holly = CornerStyle::ALL.iter().position(|s| *s == CornerStyle::Holly).unwrap();
    let src = preview_of(click_on(&mut studio, Hit::Choice(style, holly)));
    assert!(src.contains("kind = \"corners\"\nstyle = \"holly\""), "{src}");
    paint(&mut studio, &mut text, &mut scene);
    // The snow's density (the third layer's second option) to its first stop: none.
    let density = Row::Field { layer: 2, field: 1 };
    let (area, row) = studio.sliders.iter().find(|(_, r)| *r == density).cloned().unwrap();
    assert!(studio.slide(&area, row, area.rect.min_x()));
    assert!(previewed(&mut studio).contains("density = 0\n"));
    // A glow added from the gallery (it closes, the new card open), then the garland
    // removed from its menu.
    paint(&mut studio, &mut text, &mut scene);
    let _ = click_on(&mut studio, Hit::Gallery(true));
    paint(&mut studio, &mut text, &mut scene);
    let glow = LayerKind::ALL.iter().position(|k| *k == LayerKind::Glow).unwrap();
    let src = preview_of(click_on(&mut studio, Hit::Choice(Row::Add, glow)));
    assert!(src.contains("kind = \"glow\""), "{src}");
    assert!(!studio.gallery_open && studio.layer_open(3));
    paint(&mut studio, &mut text, &mut scene);
    assert!(click_on(&mut studio, Hit::Menu(0)).is_none());
    paint(&mut studio, &mut text, &mut scene);
    for item in [effects::MenuItem::Duplicate, effects::MenuItem::Reset, effects::MenuItem::Remove]
    {
        assert!(has(&studio, &Hit::MenuItem(0, item)), "{item:?}");
    }
    let src = preview_of(click_on(&mut studio, Hit::MenuItem(0, effects::MenuItem::Remove)));
    assert!(!src.contains("kind = \"garland\""), "{src}");
    assert_eq!(studio.menu_for, None);
    // A color picked for the snow (now the second layer): written in hex.
    studio.command("open-all");
    paint(&mut studio, &mut text, &mut scene);
    let key = layer_color_key(1, "colors", 0);
    assert!(click_on(&mut studio, Hit::Token(key)).is_none(), "the picker opens");
    assert!(studio.picker.is_open());
    studio.picker_action(PickerAction::Apply(Rgba::rgb(0x123456), ColorFormat::Hex));
    assert!(previewed(&mut studio).contains("colors = [\"#123456\"]"));
    // One color more, then one fewer.
    paint(&mut studio, &mut text, &mut scene);
    let src = preview_of(click_on(&mut studio, Hit::AddColor(1, "colors")));
    assert!(src.contains("colors = [\"#123456\", \"#123456\"]"), "{src}");
    paint(&mut studio, &mut text, &mut scene);
    let src = preview_of(click_on(&mut studio, Hit::DropColor(1, "colors")));
    assert!(src.contains("colors = [\"#123456\"]"), "{src}");
    // A preset: its layers.
    assert!(studio.command("preset:autumn"));
    let src = previewed(&mut studio);
    assert!(src.contains("shape = \"leaves\"") && !src.contains("shape = \"snow\""), "{src}");
    assert!(!studio.command("preset:easter"));
    assert!(studio.command("preset:none"));
    assert!(!previewed(&mut studio).contains("[[decorations"));
}

/// An effect switched off is kept (written `enabled = false`), duplicated and reset from
/// its menu; dragged by its handle it moves among the others.
#[test]
fn effects_are_switched_duplicated_reset_and_reordered() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Effects);
    let _ = studio.take_action();
    let src = preview_of(click_on(&mut studio, Hit::Toggle(0)));
    assert!(src.contains("kind = \"garland\"\nenabled = false"), "{src}");
    studio.menu_item(2, effects::MenuItem::Duplicate);
    let kinds = |s: &Studio| {
        s.draft
            .as_ref()
            .unwrap()
            .decorations
            .layers
            .iter()
            .map(|l| l.spec.kind())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(&studio),
        [LayerKind::Garland, LayerKind::Corners, LayerKind::Particles, LayerKind::Particles]
    );
    studio.draft.as_mut().unwrap().decorations.layers[1]
        .spec
        .set("opacity", &FieldValue::Number(0.1))
        .unwrap();
    studio.menu_item(1, effects::MenuItem::Reset);
    assert_eq!(
        studio.draft.as_ref().unwrap().decorations.layers[1],
        studio.base_draft.as_ref().unwrap().decorations.layers[1]
    );
    // The last effect dragged above the first.
    paint(&mut studio, &mut text, &mut scene);
    let handle = studio.hits.iter().find(|(_, h)| *h == Hit::Handle(3)).map(|(r, _)| *r).unwrap();
    let first = studio.cards_at[0].1;
    assert!(studio.click(Point::new(handle.min_x() + 2.0, handle.min_y() + 2.0)).is_none());
    assert!(studio.drag(Point::new(handle.min_x(), first.min_y() + 2.0)));
    paint(&mut studio, &mut text, &mut scene);
    assert!(studio.release());
    assert_eq!(kinds(&studio)[0], LayerKind::Particles);
    assert_eq!(kinds(&studio).len(), 4);
}

/// "Use these with every theme": the layers handed to the decorations' settings, one a
/// line, each read back as it was.
#[test]
fn the_decorations_go_to_every_theme() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Effects);
    let _ = studio.take_action();
    let action = click_on(&mut studio, Hit::Overlay);
    let Some(ViewerAction::Ask(asked)) = &action else { panic!("{action:?}") };
    let own = asked.request::<contracts::decorations::SetOwnDecorations>().expect("its layers");
    let layers: Vec<_> = own.layers.lines().map(|l| decode(l).unwrap()).collect();
    assert_eq!(layers, presets::layers("christmas", true).unwrap());
    assert!(studio.overlay_saved);
    let LayerSpec::Particles(p) = &layers[2] else { panic!() };
    assert_eq!(p.shape, ParticleShape::Snow);
}

/// The theme's own settings: chosen here, they become the saved theme's defaults (a choice,
/// a slider, a color, a switch).
#[test]
fn the_theme_s_settings_are_chosen_as_its_defaults() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Options);
    let _ = studio.take_action();
    let src = preview_of(click_on(&mut studio, Hit::Choice(Row::Option(0), 2)));
    assert!(
        src.contains(
            "id = \"lights\"\nkind = \"choice\"\nlabel.en = \"Lights\"\ndefault = \"icy\""
        ),
        "{src}"
    );
    paint(&mut studio, &mut text, &mut scene);
    let (area, row) = studio.sliders.iter().find(|(_, r)| *r == Row::Option(1)).cloned().unwrap();
    assert!(studio.slide(&area, row, area.rect.max_x()));
    assert!(previewed(&mut studio).contains("default = 0.8"));
    // A color setting: its swatch opens the picker.
    paint(&mut studio, &mut text, &mut scene);
    let key = theming::studio::option_color_key("tint");
    assert!(click_on(&mut studio, Hit::Token(key)).is_none());
    studio.picker_action(PickerAction::Apply(Rgba::rgb(0xff0000), ColorFormat::Hex));
    assert!(previewed(&mut studio).contains("default = \"#ff0000\""));
    // A switch: off.
    paint(&mut studio, &mut text, &mut scene);
    let src = preview_of(click_on(&mut studio, Hit::Choice(Row::Option(3), 0)));
    assert!(
        src.contains("id = \"bows\"\nkind = \"toggle\"\nlabel.en = \"Bows\"\ndefault = false"),
        "{src}"
    );
}

/// Changes are seen: counted, a dot on their category, "↺" on their row (which puts it
/// back); "Cancel" drops them all, brings the window's theme back and closes the studio at
/// once ("Close" with none, the same).
#[test]
fn changes_are_counted_reset_and_discarded() {
    let (mut studio, mut text, mut scene) = opened();
    assert_eq!(studio.changes(), 0);
    show(&mut studio, &mut text, &mut scene, Category::Interface);
    assert!(!has(&studio, &Hit::ResetToken("accent".into())), "nothing to put back");
    studio.draft.as_mut().unwrap().set("accent", Rgba::rgb(0x00ff00));
    studio.draft.as_mut().unwrap().set("syntax.string", Rgba::rgb(0x00ff00));
    studio.draft.as_mut().unwrap().caret.width = Some(4.0);
    assert_eq!(studio.changes(), 3);
    let (draft, base) = (studio.draft.clone().unwrap(), studio.base_draft.clone().unwrap());
    let entries = studio.entries();
    for (c, dot) in [
        (Category::Interface, true),
        (Category::Syntax, true),
        (Category::Caret, true),
        (Category::Effects, false),
    ] {
        assert_eq!(model::category_changed(&entries, &draft, &base, c), dot, "{c:?}");
    }
    paint(&mut studio, &mut text, &mut scene);
    let _ = click_on(&mut studio, Hit::ResetToken("accent".into()));
    assert_eq!(studio.draft.as_ref().unwrap().get("accent"), base.get("accent"));
    assert_eq!(studio.changes(), 2);
    // A layer removed counts too.
    studio.remove_layer(0);
    assert_eq!(studio.changes(), 2 + 2 + 1, "the two shifted into its place, and one removed");
    paint(&mut studio, &mut text, &mut scene);
    let cancelled = click_on(&mut studio, Hit::Discard);
    assert_eq!(cancelled, Some(ViewerAction::Request(AppRequest::CancelTheme)), "one press");
    assert_eq!(studio.changes(), 0);
    assert_eq!(studio.draft, studio.base_draft);
    paint(&mut studio, &mut text, &mut scene);
    let closed = click_on(&mut studio, Hit::Discard);
    assert_eq!(closed, Some(ViewerAction::Request(AppRequest::CancelTheme)));
}

/// The search finds a setting across the categories (any case, without diacritics) and
/// shows it: its category, scrolled to its row, highlighted.
#[test]
fn the_search_finds_and_shows_a_setting() {
    let (mut studio, mut text, mut scene) = opened();
    let trail = Row::Caret(CaretRow::Trail).label(studio.draft.as_ref().unwrap()).to_uppercase();
    assert!(studio.command(&format!("search:{trail}")));
    paint_at(&mut studio, &mut text, &mut scene, 1400.0, 700.0);
    assert!(has(&studio, &Hit::Found(0)));
    assert!(!has(&studio, &Hit::Found(1)), "only the caret's trail");
    let _ = click_on(&mut studio, Hit::Found(0));
    assert_eq!(studio.category, Category::Caret);
    assert!(studio.query.text().is_empty());
    paint_at(&mut studio, &mut text, &mut scene, 1400.0, 700.0);
    assert_eq!(studio.highlight, Some(SettingId::Caret(CaretRow::Trail)));
    let (_, r) =
        studio.rows_at.iter().find(|(id, _)| *id == SettingId::Caret(CaretRow::Trail)).unwrap();
    assert!(studio.list.contains(r.origin), "scrolled to it: {r:?}");
    // A theme's name finds the themes to start from.
    assert!(studio.command("search:light!"));
    paint(&mut studio, &mut text, &mut scene);
    assert_eq!(studio.category, Category::General);
}

/// Narrow: the categories are tabs over the panel, the preview hides behind a button that
/// shows it over the panel.
#[test]
fn a_narrow_studio_tabs_its_categories_and_hides_its_preview() {
    let (mut studio, mut text, mut scene) = opened();
    paint_at(&mut studio, &mut text, &mut scene, 700.0, 900.0);
    let frame = layout::Frame::of(Rect::new(0.0, 0.0, 700.0, 900.0), 1.0);
    assert!(frame.nav.is_none() && frame.preview.is_none());
    assert_eq!(frame.panel.size.width, 700.0);
    // Medium: tabs, the preview beside.
    let medium = layout::Frame::of(Rect::new(0.0, 0.0, 900.0, 900.0), 1.0);
    assert!(medium.nav.is_none() && medium.preview.is_some());
    let tabs: Vec<Rect> = studio
        .hits
        .iter()
        .filter(|(_, h)| matches!(h, Hit::Category(_)))
        .map(|(r, _)| *r)
        .collect();
    assert_eq!(tabs.len(), 6);
    assert!(tabs.iter().all(|r| r.min_y() < studio.list.min_y()), "above the panel");
    assert!(has(&studio, &Hit::PreviewOpen));
    let _ = click_on(&mut studio, Hit::PreviewOpen);
    assert!(studio.preview_open);
    // Wide: the list down the side and the preview beside.
    let wide = layout::Frame::of(Rect::new(0.0, 0.0, 1400.0, 900.0), 1.0);
    assert!(wide.nav.is_some() && wide.preview.is_some());
    assert!(wide.panel.size.width >= 500.0);
}

/// A slider clicked: the arrows step it, Shift by more, never past its ends.
#[test]
fn the_keys_step_the_slider_clicked_last() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Caret);
    let row = Row::Caret(CaretRow::Trail);
    let (area, _) = studio.sliders.iter().find(|(_, r)| *r == row).cloned().unwrap();
    studio.click(Point::new(area.stops[0], area.rect.min_y() + 4.0));
    studio.release();
    assert_eq!(row.current(studio.draft.as_ref().unwrap()), 0);
    assert!(studio.edit_key(ViewerKey::Right));
    assert_eq!(row.current(studio.draft.as_ref().unwrap()), 1);
    assert!(
        studio
            .edit_key_with(ViewerKey::Right, ui_components::KeyReach { word: false, extend: true })
    );
    assert_eq!(row.current(studio.draft.as_ref().unwrap()), 3);
    for _ in 0..9 {
        studio.edit_key(ViewerKey::Right);
    }
    assert_eq!(row.current(studio.draft.as_ref().unwrap()), 4, "its last stop");
    assert!(studio.edit_key(ViewerKey::Escape));
    assert!(!studio.edit_key(ViewerKey::Left), "let go");
}

/// The gallery of themes to start from: filtered by appearance; one chosen becomes the
/// draft (and the theme in use is marked).
#[test]
fn the_themes_to_start_from_are_filtered_and_chosen() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::General);
    let _ = click_on(&mut studio, Hit::BaseFilter(BaseFilter::Light));
    paint(&mut studio, &mut text, &mut scene);
    assert!(has(&studio, &Hit::Base(2)) && has(&studio, &Hit::Base(3)));
    assert!(!has(&studio, &Hit::Base(0)) && !has(&studio, &Hit::Base(1)));
    let src = preview_of(click_on(&mut studio, Hit::Base(2)));
    assert!(src.contains("appearance = \"light\""), "{src}");
    assert_eq!(studio.base, 2);
    assert_eq!(studio.changes(), 0, "a new start: nothing changed");
}

/// The preview of a theme with both variants shows either; one without, only its own.
#[test]
fn the_preview_shows_the_other_variant_of_a_theme_with_both() {
    let (mut studio, mut text, mut scene) = opened();
    assert_eq!(studio.sibling(studio.input.as_ref().unwrap()), Some(3));
    assert!(has(&studio, &Hit::PreviewLight(true)));
    let _ = click_on(&mut studio, Hit::PreviewLight(true));
    assert!(studio.preview_light);
    studio.load_base(1);
    assert!(!studio.preview_light, "a new start shows its own");
    assert_eq!(studio.sibling(studio.input.as_ref().unwrap()), Some(2));
    let alone = studio_with(("ocean", "Ocean"));
    assert_eq!(alone.sibling(alone.input.as_ref().unwrap()), None);
    let _ = (&mut text, &mut scene);
}

/// The decorations' provider the tests register: it previews whatever it's given (a
/// painter drawing nothing), at 20 frames a second.
#[derive(Debug)]
struct Previewer;

#[derive(Debug)]
struct Nothing(Arc<theming::decorations::Decorations>);

impl ui_components::decorations::WindowDecorations for Nothing {
    fn paint(&mut self, _: &ui_components::decorations::DecorationFrame<'_>, _: &mut Layer) {}
    fn moves(&self) -> bool {
        self.0.moves()
    }
    fn memory(&self) -> usize {
        0
    }
    fn gpu_memory(&self) -> u64 {
        0
    }
}

impl ui_components::decorations::DecorationsProvider for Previewer {
    fn create(
        &self,
        d: &Arc<theming::decorations::Decorations>,
    ) -> Option<Box<dyn ui_components::decorations::WindowDecorations>> {
        Some(Box::new(Nothing(Arc::clone(d))))
    }
    fn mode(&self) -> ui_components::decorations::DecorationMode {
        ui_components::decorations::DecorationMode::On
    }
    fn version(&self) -> u64 {
        0
    }
}

/// "Performance and accessibility": the decorations' own settings, each handed to their
/// block (saved for every theme); the estimated cost shown. The preview's effects move
/// (frames at a loader's pace) only after input and while not asked to stand still.
#[test]
fn the_effects_performance_is_set_and_the_preview_moves_only_when_asked() {
    ui_components::decorations::register(Arc::new(Previewer));
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Effects);
    for g in model::Global::ALL {
        assert!(studio.rows_at.iter().any(|(id, _)| *id == SettingId::Global(g)), "{g:?}");
    }
    let _ = studio.take_action();
    let action = click_on(&mut studio, Hit::Global(model::Global::Fps, 30));
    use contracts::decorations::{DecorationPace, SetDecorationPace};
    assert_eq!(
        action,
        Some(ViewerAction::Ask(ui_components::Asked::new(SetDecorationPace {
            what: DecorationPace::Fps,
            value: 30
        })))
    );
    let action = click_on(&mut studio, Hit::Global(model::Global::ReduceMotion, 0));
    assert_eq!(
        action,
        Some(ViewerAction::Ask(ui_components::Asked::new(SetDecorationPace {
            what: DecorationPace::FollowSystem,
            value: 0
        })))
    );
    // The preview: still until the user does something, then moving; still when asked.
    assert!(studio.live.is_some(), "its decorations made");
    studio.last_input = None;
    assert!(!studio.is_loading(), "no input (or none for a while)");
    studio.hover(Point::new(1.0, 1.0));
    assert!(studio.is_loading());
    let _ = click_on(&mut studio, Hit::PreviewMoving(false));
    assert!(!studio.is_loading());
    // Let go (the caches' purge).
    studio.purge();
    assert!(studio.live.is_none());
}

/// Every kind, option and choice has its name and description in the locales (none shown
/// as its key).
#[test]
fn every_effect_is_named_in_the_locales() {
    let named = |s: String| assert!(!s.starts_with("studio.") && !s.is_empty(), "{s}");
    for kind in LayerKind::ALL {
        named(rows::kind_label(*kind));
        named(t(&format!("studio.kind_{}_detail", kind.id().replace('-', "_"))));
        for f in kind.fields() {
            named(t(&format!("studio.field_{}", f.key)));
            named(t(&format!("studio.field_{}_detail", f.key)));
            if let theming::decorations::FieldKind::Choice(ids) = f.kind {
                for id in ids {
                    named(rows::value_label(id));
                }
            }
        }
    }
    for id in presets::ALL {
        let name = t(&format!("decorations.preset_{}", id.replace('-', "_")));
        assert!(!name.starts_with("decorations."), "{id}");
    }
}

/// The backdrop's section (in Interface): its style, tint and blur chosen go into the theme
/// previewed, each with its "↺", the preview showing a dialog on it; a reset brings back
/// the starting theme's.
#[test]
fn the_backdrop_is_set_in_the_studio() {
    use rows::BackdropRow;
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Interface);
    assert!(studio.rows_at.iter().any(|(id, _)| *id == SettingId::BackdropColor));
    let style = Row::Backdrop(BackdropRow::Style);
    let blur = Row::Backdrop(BackdropRow::Blur);
    // The starting theme's is Glass (every bundled theme's): Simple chosen.
    assert!(studio.command("backdrop:simple"));
    assert!(studio.backdrop_shown);
    assert!(previewed(&mut studio).contains("style   = \"simple\""));
    assert!(studio.set_stop(blur, 7));
    assert!(studio.set_stop(Row::Backdrop(BackdropRow::Opacity), 0));
    let src = previewed(&mut studio);
    let section = src.split("\n[backdrop]\n").nth(1).unwrap().split("\n[").next().unwrap();
    assert!(section.contains("blur    = 7") && !section.contains("opacity"), "{section}");
    let (draft, base) = (studio.draft.clone().unwrap(), studio.base_draft.clone().unwrap());
    assert!(rows::differs(style, &draft, &base) && rows::differs(blur, &draft, &base));
    assert!(model::modified(&draft, &base, &SettingId::Backdrop(BackdropRow::Blur)));
    paint(&mut studio, &mut text, &mut scene);
    assert!(has(&studio, &Hit::ResetRow(blur)));
    studio.reset_row(blur);
    assert_eq!(studio.draft.as_ref().unwrap().backdrop.blur, None);
    // Another category: the preview's dialog goes.
    let _ = click_on(&mut studio, Hit::Category(Category::Syntax));
    assert!(!studio.backdrop_shown);
}

/// The menu panel's section (in Interface, after the backdrop's): its style, opacity and blur
/// chosen go into the theme previewed (`[floating]`), each with its "↺", the preview's dialog
/// shown; a reset brings back the starting theme's.
#[test]
fn the_floating_glass_is_set_in_the_studio() {
    use rows::FloatingRow;
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Interface);
    let opacity = Row::Floating(FloatingRow::Opacity);
    assert!(studio.rows_at.iter().any(|(id, _)| *id == SettingId::Floating(FloatingRow::Opacity)));
    assert!(studio.set_stop(opacity, 3));
    assert!(studio.backdrop_shown, "the preview's dialog shows it");
    let src = previewed(&mut studio);
    let section = src.split("\n[floating]\n").nth(1).unwrap().split("\n[").next().unwrap();
    assert!(section.contains("opacity = 0.75") && section.contains("glass"), "{section}");
    let (draft, base) = (studio.draft.clone().unwrap(), studio.base_draft.clone().unwrap());
    assert!(model::modified(&draft, &base, &SettingId::Floating(FloatingRow::Opacity)));
    paint(&mut studio, &mut text, &mut scene);
    assert!(has(&studio, &Hit::ResetRow(opacity)));
    studio.reset_row(opacity);
    assert_eq!(studio.draft.as_ref().unwrap().floating.opacity, None);
}

/// A window too short for the settings: once they scroll (the bar shows), its thumb, pressed
/// and dragged, scrolls them.
#[test]
fn the_settings_scroll_by_their_thumb() {
    let (mut studio, mut text, mut scene) = opened();
    let (w, h) = (1400.0, 600.0);
    paint_at(&mut studio, &mut text, &mut scene, w, h);
    // Hidden at rest: a scroll shows it.
    assert!(studio.scroll(0.0, 1.0));
    paint_at(&mut studio, &mut text, &mut scene, w, h);
    let list = studio.list;
    let thumb = Point::new(list.max_x() - 4.0, list.min_y() + 6.0);
    assert_eq!(studio.click(thumb), None);
    let pressed = studio.scrolled();
    assert!(studio.drag(Point::new(thumb.x, thumb.y + 40.0)));
    assert!(studio.scrolled() > pressed, "scrolled down");
    assert!(studio.release());
}

/// The name's field: pressed, it has the keys (only ⌫ ← → edit it); ↵ lets go of them, as a
/// press elsewhere does.
#[test]
fn the_name_takes_the_keys_until_enter() {
    let (mut studio, _text, _scene) = opened();
    let _ = click_on(&mut studio, Hit::Name);
    assert!(studio.editing_name() && studio.wants_text());
    assert!(studio.text_input("!"));
    assert!(studio.name.text().starts_with('!'), "the caret where it was pressed");
    assert!(!studio.edit_key(ViewerKey::Tab), "Tab isn't the studio's");
    assert!(studio.edit_key(ViewerKey::Enter));
    assert!(!studio.editing_name() && !studio.wants_text());
    let _ = click_on(&mut studio, Hit::Name);
    assert!(studio.command("select_all"));
    let _ = click_on(&mut studio, Hit::Category(Category::Caret));
    assert!(!studio.editing_name(), "a press elsewhere lets go");
    assert!(!studio.command("select_all"));
}

/// An open effect's menu: a press elsewhere only closes it (the search keeps the keys).
#[test]
fn a_press_beside_an_open_menu_only_closes_it() {
    let (mut studio, _text, _scene) = opened();
    assert!(studio.command("find"));
    studio.menu_for = Some(0);
    let before = studio.category;
    let _ = click_on(&mut studio, Hit::Category(Category::Caret));
    assert_eq!(studio.menu_for, None);
    assert_eq!(studio.category, before, "the press did nothing more");
    assert!(studio.editing_query(), "the search kept the keys");
    // Esc: the search emptied and let go.
    assert!(studio.text_input("x"));
    assert!(studio.edit_key(ViewerKey::Escape));
    assert!(studio.query.text().is_empty() && !studio.editing_query());
}

/// The color picker has the keys while it's open; closed, they go back where they were.
#[test]
fn the_picker_has_the_keys_while_open() {
    let (mut studio, mut text, mut scene) = opened();
    show(&mut studio, &mut text, &mut scene, Category::Interface);
    let token = studio.hits.iter().find_map(|(_, h)| match h {
        Hit::Token(key) => Some(Hit::Token(key.clone())),
        _ => None,
    });
    let _ = click_on(&mut studio, token.expect("a color to pick"));
    assert!(studio.picker.is_open() && studio.wants_text());
    // The search asked for meanwhile: its turn comes once the picker closes.
    assert!(studio.command("find"));
    assert!(studio.text_input("1"), "the picker's field takes it");
    assert!(studio.query.text().is_empty());
    assert!(studio.editing_query(), "the focus stays under the picker");
    assert!(studio.command("select_all"), "⌘A: the field's under it, not the picker's");
    assert!(!studio.edit_key(ViewerKey::Down), "a key the picker doesn't use isn't the field's");
    assert!(studio.edit_key(ViewerKey::Escape), "the picker gives up");
    assert!(!studio.picker.is_open());
    assert!(studio.editing_query());
}
