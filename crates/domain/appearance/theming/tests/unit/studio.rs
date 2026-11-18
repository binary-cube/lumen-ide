use super::*;

const TEMPLATE: &str = r##"# Baza
id = "base"
name = "Baza"
appearance = "dark"

[palette]
base      = "#101010"
card      = "#202020"
panel     = "#303030"
line      = "#404040"
fg        = "#e0e0e0"
muted     = "#808080"
code      = "#c0c0c0"
accent    = "#3574f0"
green     = "#00aa00"

[ui]
"editor.background" = "$card"
"tab.hover"         = "#282828"
"toggle.active"     = "#3574f059"

[syntax]
"comment"  = { fg = "$muted", italic = true }
"string"   = "$green"
"function" = "#5599ff"
"function.method" = "#5599ff"
"##;

#[test]
fn read_and_compose() {
    let mut draft = read_draft(TEMPLATE).unwrap();
    assert_eq!(draft.get("syntax.string"), Rgba::from_hex("#00aa00"));
    draft.id = "mine".into();
    draft.name = "A mea".into();
    draft.set("card", Rgba::from_hex("#ffffff").unwrap());
    draft.set("fg", Rgba::from_hex("#000000").unwrap());
    draft.set("accent", Rgba::from_hex("#ff0000").unwrap());
    draft.set("syntax.comment", Rgba::from_hex("#123456").unwrap());
    draft.set("syntax.function", Rgba::from_hex("#abcdef").unwrap());
    let out = compose(TEMPLATE, &draft).unwrap();
    assert!(out.contains("id = \"mine\"") && out.contains("appearance = \"light\""), "{out}");
    assert!(out.contains("card      = \"#ffffff\""));
    // The translucent accent in the UI follows the new accent, with alpha kept.
    assert!(out.contains("\"#ff000059\""), "{out}");
    // The comment style stays, the color is new; methods follow functions.
    assert!(out.contains("\"comment\" = { fg = \"#123456\", italic = true }"), "{out}");
    assert!(out.contains("\"function.method\" = \"#abcdef\""), "{out}");
    // And it can be read again.
    assert_eq!(read_draft(&out).unwrap().get("syntax.function"), Rgba::from_hex("#abcdef"));
}

/// A theme's decorations and own settings come read (not from its text); `compose` writes
/// them in their own sections after the syntax (never mixed into it), the values that follow
/// the palette or an option as `$name` / `@id`; the layers' colors are set by key, and one
/// set by hand follows nothing any more. A theme without them gets none.
#[test]
fn decorations_and_options_are_composed() {
    use crate::decorations::{
        BindTo, Binding, Condition, DecorationLayer, LayerKind, LayerSpec, Merge,
    };
    use crate::options::{Label, OptionChoice, OptionKind, OptionValue, ThemeOption};
    let snow = DecorationLayer {
        spec: LayerSpec::new(LayerKind::Particles),
        when: Condition::parse("snowing"),
        bindings: vec![
            Binding { key: "density", index: 0, to: BindTo::Option("amount".into()) },
            Binding { key: "colors", index: 0, to: BindTo::Palette("green".into()) },
        ],
        enabled: true,
    };
    let theme = StudioTheme {
        decorations: DecorationsSpec {
            layers: vec![DecorationLayer::plain(LayerSpec::new(LayerKind::Garland)), snow],
            merge: Merge::Replace,
        },
        options: vec![
            ThemeOption {
                id: "amount".into(),
                label: Label(vec![
                    ("en".into(), "Snow \"amount\"".into()),
                    ("ro".into(), "Zăpadă".into()),
                ]),
                kind: OptionKind::Slider { min: 0.0, max: 0.8, step: 0.1 },
                default: OptionValue::Number(0.4),
            },
            ThemeOption {
                id: "tone".into(),
                label: Label::plain("Tone"),
                kind: OptionKind::Choice(vec![OptionChoice {
                    id: "warm".into(),
                    label: Label::plain("Warm"),
                    palette: vec![("green".into(), Rgba::rgb(0x88aa00))],
                }]),
                default: OptionValue::Choice("warm".into()),
            },
        ],
        ..StudioTheme::plain("base", "Baza", TEMPLATE)
    };
    let mut draft = theme.draft().unwrap();
    assert_eq!(draft.get("syntax.string"), Rgba::from_hex("#00aa00"));
    let key = layer_color_key(1, "colors", 0);
    assert_eq!(draft.get(&key), Some(Rgba::WHITE));
    draft.id = "mine".into();
    let out = compose(TEMPLATE, &draft).unwrap();
    let syntax = out.split("\n[syntax]\n").nth(1).unwrap();
    let syntax = syntax.split("\n[[").next().unwrap();
    assert!(!syntax.contains("kind"), "{out}");
    let options_at = out.find("[[options]]").unwrap();
    let layers_at = out.find("[[decorations.layers]]").unwrap();
    assert!(options_at < layers_at, "the options before what follows them");
    assert_eq!(out.matches("[[decorations.layers]]").count(), 2, "{out}");
    assert!(out.contains("kind = \"particles\"\nwhen = \"snowing\""), "{out}");
    assert!(out.contains("density = \"@amount\""), "{out}");
    assert!(out.contains("colors = [\"$green\"]"), "{out}");
    assert!(out.contains("label.en = \"Snow \\\"amount\\\"\"\nlabel.ro = \"Zăpadă\""), "{out}");
    assert!(out.contains("min = 0\nmax = 0.8\nstep = 0.1"), "{out}");
    assert!(
        out.contains(
            "[[options.choices]]\nid = \"warm\"\nlabel.en = \"Warm\"\npalette.green = \"#88aa00\""
        ),
        "{out}"
    );
    // A color set by hand: its own, written in hex.
    draft.set(&key, Rgba::rgb(0x123456));
    assert_eq!(draft.get(&key), Some(Rgba::rgb(0x123456)));
    assert!(draft.decorations.layers[1].binding("colors", 0).is_none());
    let out = compose(TEMPLATE, &draft).unwrap();
    assert!(out.contains("colors = [\"#123456\"]"), "{out}");
    // A color setting's value.
    draft.options.push(ThemeOption {
        id: "tint".into(),
        label: Label::plain("Tint"),
        kind: OptionKind::Color { palette: Some("accent".into()) },
        default: OptionValue::Color(Rgba::WHITE),
    });
    draft.set(&option_color_key("tint"), Rgba::rgb(0xff0000));
    assert_eq!(draft.get(&option_color_key("tint")), Some(Rgba::rgb(0xff0000)));
    assert!(
        compose(TEMPLATE, &draft).unwrap().contains("default = \"#ff0000\"\npalette = \"accent\"")
    );
    // A theme without them: none written.
    let plain = read_draft(TEMPLATE).unwrap();
    assert!(plain.decorations.is_empty() && plain.options.is_empty());
    let out = compose(TEMPLATE, &plain).unwrap();
    assert!(
        !out.contains("[decorations")
            && !out.contains("[[decorations")
            && !out.contains("[[options")
    );
}

/// The state colors are editable where the palette has them (the template has a success
/// color, no error or warning one): read, written back in `[palette]`, and the tokens
/// written `$green` follow.
#[test]
fn state_colors_are_edited_where_the_palette_has_them() {
    let mut draft = read_draft(TEMPLATE).unwrap();
    assert_eq!(draft.get("green"), Rgba::from_hex("#00aa00"));
    assert_eq!(draft.get("red"), None);
    draft.set("green", Rgba::from_hex("#11cc11").unwrap());
    let out = compose(TEMPLATE, &draft).unwrap();
    assert!(out.contains("green     = \"#11cc11\""), "{out}");
    assert_eq!(read_draft(&out).unwrap().get("green"), Rgba::from_hex("#11cc11"));
}

/// The UI value of `key` in a composed theme's text (its `[ui]` line, quotes cut).
fn ui_value(src: &str, key: &str) -> String {
    let line = src
        .lines()
        .find(|l| l.trim_start().starts_with(&format!("\"{key}\"")))
        .unwrap_or_else(|| panic!("no {key} in {src}"));
    quoted(line.split_once('=').unwrap().1).unwrap().to_owned()
}

/// A theme tinted in its accent's hue (Matrix: near-black greens, a green accent): its dark
/// UI colors are backgrounds, not accents — HSL calls a near-black green "saturated", which
/// once turned the side panels' background into the bright accent (a regression). Composed
/// unchanged (effects added), the theme's own colors stay as they were; with a new
/// background, they follow it and stay dark.
#[test]
fn a_theme_tinted_in_its_accents_hue_keeps_its_dark_backgrounds() {
    let matrix = include_str!("../../../../../../themes/matrix.toml");
    let mut draft = read_draft(matrix).unwrap();
    draft.id = "mine".into();
    let out = compose(matrix, &draft).unwrap();
    for (key, was) in [
        ("sidebar.background", "#020803"),
        ("card.border", "#0a1f10"),
        ("tree.selection", "#082513"),
        ("toolStrip.hover", "#0a2213"),
        ("searchField.background", "#020803"),
    ] {
        assert_eq!(ui_value(&out, key).to_lowercase(), was, "{key} unchanged");
    }
    // A new (dark blue) background: the tinted backgrounds follow it, still dark.
    draft.set("card", Rgba::from_hex("#0a0f1e").unwrap());
    let out = compose(matrix, &draft).unwrap();
    for key in ["sidebar.background", "card.border", "tree.selection", "toolStrip.hover"] {
        let c = Rgba::from_hex(&ui_value(&out, key)).unwrap();
        assert!(luminance(c) < 0.2, "{key} stays dark: {c:?}");
    }
}

/// A theme's backdrop is read into the draft (its `$name` color through the palette) and
/// written back in its own section; a theme without one gets none until it's changed.
#[test]
fn the_backdrop_is_read_and_composed() {
    use crate::backdrop::{BackdropKind, BackdropSpec};
    // Neon without the glass every bundled theme asks for (55k): a theme saying nothing.
    let bundled = include_str!("../../../../../../themes/neon.toml");
    let neon = &bundled[..bundled.find("\n# The windows over the editor").unwrap()];
    let mut draft = read_draft(neon).unwrap();
    assert_eq!(draft.backdrop, BackdropSpec::default());
    assert_eq!(draft.get(BACKDROP_COLOR), Some(Rgba::BLACK), "black while unsaid");
    draft.id = "mine".into();
    assert!(!compose(neon, &draft).unwrap().contains("[backdrop]"));
    draft.backdrop.style = Some(BackdropKind::Glass);
    draft.backdrop.opacity = Some(0.25);
    draft.backdrop.blur = Some(6.0);
    draft.set(BACKDROP_COLOR, Rgba::rgb(0x102030).with_alpha(0.4));
    let out = compose(neon, &draft).unwrap();
    let again = read_draft(&out).unwrap();
    assert_eq!(again.backdrop, draft.backdrop);
    assert_eq!(again.backdrop.color, Some(Rgba::rgb(0x102030)));
    // Through the palette.
    let src = out.replace("color   = \"#102030\"", "color   = \"$accent\"");
    let accent = read_draft(&src).unwrap().get("accent");
    assert_eq!(read_draft(&src).unwrap().backdrop.color, accent);
}

/// The floating panels' glass (`[floating]`) is read into the draft and written back in its
/// own section, after the backdrop's; the bundled themes ask for glass in both.
#[test]
fn the_floating_glass_is_read_and_composed() {
    use crate::backdrop::{BackdropKind, SurfaceSpec};
    let bundled = include_str!("../../../../../../themes/neon.toml");
    let draft = read_draft(bundled).unwrap();
    assert_eq!(draft.backdrop.style, Some(BackdropKind::Glass));
    assert_eq!(draft.floating.style, Some(BackdropKind::Glass));
    let neon = &bundled[..bundled.find("\n# The windows over the editor").unwrap()];
    let mut draft = read_draft(neon).unwrap();
    assert_eq!(draft.floating, SurfaceSpec::default());
    draft.id = "mine".into();
    assert!(!compose(neon, &draft).unwrap().contains("[floating]"));
    draft.floating.style = Some(BackdropKind::Glass);
    draft.floating.opacity = Some(0.85);
    draft.floating.blur = Some(5.0);
    let out = compose(neon, &draft).unwrap();
    assert_eq!(read_draft(&out).unwrap().floating, draft.floating);
    // Out of the panel's range (it must stay readable): not read.
    let src = out.replace("opacity = 0.85", "opacity = 0.2");
    assert_eq!(read_draft(&src).unwrap().floating.opacity, None);
}
