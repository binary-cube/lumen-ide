use super::*;
use theming::ThemeRegistry;

#[test]
fn builtin_themes_parse_and_define_core_tokens() {
    let themes = builtin_themes();
    assert_eq!(themes.len(), BUILTIN.len());
    let mut registry = ThemeRegistry::default();
    for t in themes {
        registry.register(t);
    }
    for t in registry.themes().to_vec() {
        let r = registry.resolve(&t.id).unwrap();
        for token in ["window.background", "editor.background", "text", "accent", "tree.selection"]
        {
            assert!(r.color(token).is_some(), "{} doesn't define {token}", t.id);
        }
        assert!(r.syntax("keyword").and_then(|s| s.foreground).is_some());
        assert!(r.syntax("comment").is_some_and(|s| s.italic));
    }
}

#[test]
fn palette_references_and_shorthand_syntax() {
    let t = parse(
        r##"
            id = "x"
            name = "X"
            appearance = "dark"
            extends = "default-dark"
            [palette]
            red = "#ff0000"
            [ui]
            "accent" = "$red"
            [syntax]
            "string" = "#00ff00"
            "comment" = { fg = "$red", italic = true }
            "##,
    )
    .unwrap();
    assert_eq!(t.ui["accent"], Rgba::rgb(0xff0000));
    assert_eq!(t.syntax["string"].foreground, Some(Rgba::rgb(0x00ff00)));
    assert!(t.syntax["comment"].italic);
    assert_eq!(t.extends, Some(ThemeId("default-dark".into())));
}

#[test]
fn invalid_values_are_reported() {
    let base = "id = \"x\"\nname = \"X\"\n";
    assert!(matches!(
        parse(&format!("{base}appearance = \"blue\"")),
        Err(ThemeFileError::Appearance(_))
    ));
    assert!(matches!(
        parse(&format!("{base}appearance = \"dark\"\n[ui]\n\"accent\" = \"$nope\"")),
        Err(ThemeFileError::Color { .. })
    ));
    assert!(matches!(parse("not toml ["), Err(ThemeFileError::Syntax(_))));
}

#[test]
fn every_builtin_theme_can_be_recomposed_in_the_studio() {
    for src in builtin_sources() {
        let mut draft = theming::studio::read_draft(src).expect("culorile editabile");
        draft.id = format!("{}-copy", draft.id);
        draft.set("accent", kernel::Rgba::rgb(0xff3366));
        let out = theming::studio::compose(src, &draft).expect("compunere");
        let theme = parse(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(theme.id.0, draft.id);
    }
}

/// A theme's `[caret]`: read (the Matrix one), a wrong value said; without it, nothing.
#[test]
fn the_caret_section_is_read() {
    let matrix = BUILTIN.iter().find(|(f, _)| *f == "matrix.toml").unwrap().1;
    let caret = parse(matrix).unwrap().caret;
    assert_eq!(caret.blink, Some(Blink::Smooth));
    assert_eq!(caret.motion, Some(Motion::Glide));
    assert_eq!(caret.trail_ms, Some(220));
    let neon = BUILTIN.iter().find(|(f, _)| *f == "neon.toml").unwrap().1;
    assert_eq!(parse(neon).unwrap().caret, CaretSpec::default());
    let wrong = "id = \"x\"\nname = \"x\"\nappearance = \"dark\"\n[caret]\nblink = \"sometimes\"\n";
    assert!(matches!(parse(wrong), Err(ThemeFileError::Caret { key: "blink", .. })));
}

/// A theme's `[backdrop]`: read (its color through the palette), a wrong value said;
/// without it, nothing.
#[test]
fn the_backdrop_section_is_read() {
    let src = r##"
        id = "x"
        name = "X"
        appearance = "dark"
        [palette]
        deep = "#102030"
        [backdrop]
        style = "glass"
        color = "$deep"
        opacity = 0.3
        blur = 3
        "##;
    let spec = parse(src).unwrap().backdrop;
    assert_eq!(spec.style, Some(BackdropKind::Glass));
    assert_eq!(spec.color, Some(Rgba::rgb(0x102030)));
    assert_eq!(spec.opacity, Some(0.3));
    assert_eq!(spec.blur, Some(3.0));
    // Every bundled theme asks for glass (gently: the blur, the tint its own), for the
    // backdrop and the floating panels alike (55k).
    for (file, src) in BUILTIN {
        let theme = parse(src).unwrap();
        assert_eq!(theme.backdrop.style, Some(BackdropKind::Glass), "{file}");
        assert_eq!(theme.floating.style, Some(BackdropKind::Glass), "{file}");
    }
    let floating = "id = \"x\"\nname = \"x\"\nappearance = \"dark\"\n[floating]\nstyle = \"glass\"\nopacity = 0.88\nblur = 6\n";
    let spec = parse(floating).unwrap().floating;
    assert_eq!(
        (spec.style, spec.opacity, spec.blur),
        (Some(BackdropKind::Glass), Some(0.88), Some(6.0))
    );
    let unreadable = floating.replace("opacity = 0.88", "opacity = 0.3");
    assert!(parse(&unreadable).is_err(), "a panel that translucent isn't readable");
    let base = "id = \"x\"\nname = \"x\"\nappearance = \"dark\"\n[backdrop]\n";
    for (wrong, key) in
        [("style = \"fog\"", "style"), ("opacity = 1.5", "opacity"), ("blur = 40", "blur")]
    {
        let err = parse(&format!("{base}{wrong}\n"));
        assert!(
            matches!(err, Err(ThemeFileError::Backdrop { key: k, .. }) if k == key),
            "{wrong}: {err:?}"
        );
    }
}

/// A theme made in the configurator from Matrix keeps its caret (in its own section, not in
/// the palette), and one made from a plain theme gets the caret chosen.
#[test]
fn themes_made_in_the_configurator_keep_the_caret() {
    use theming::studio::{compose, read_draft};
    let matrix = BUILTIN.iter().find(|(f, _)| *f == "matrix.toml").unwrap().1;
    let mut draft = read_draft(matrix).unwrap();
    assert_eq!(draft.caret.glow, Some(0.8));
    draft.id = "custom-green".into();
    let made = parse(&compose(matrix, &draft).unwrap()).unwrap();
    assert_eq!(made.caret, parse(matrix).unwrap().caret);
    let neon = BUILTIN.iter().find(|(f, _)| *f == "neon.toml").unwrap().1;
    let mut draft = read_draft(neon).unwrap();
    draft.id = "custom-glow".into();
    draft.caret.shape = Some(CaretShape::Block);
    draft.caret.glow = Some(0.5);
    let made = parse(&compose(neon, &draft).unwrap()).unwrap();
    assert_eq!((made.caret.shape, made.caret.glow), (Some(CaretShape::Block), Some(0.5)));
}

/// WCAG's contrast ratio between two opaque colors (sRGB, 1 to 21).
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let channel = |c: f32| {
        if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let luminance = |c: Rgba| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn every_builtin_theme_reads_well_in_the_editor() {
    let mut registry = ThemeRegistry::default();
    for t in builtin_themes() {
        registry.register(t);
    }
    for t in registry.themes().to_vec() {
        let r = registry.resolve(&t.id).unwrap();
        let background = r.color("editor.background").unwrap();
        let text = r.color("editor.foreground").unwrap();
        assert!(contrast(text, background) >= 4.5, "{}: the code's text is too faint", t.id);
        let muted = r.color("text.muted").unwrap();
        assert!(contrast(muted, background) >= 2.8, "{}: the muted text is too faint", t.id);
        for (scope, least) in [
            ("keyword", 2.8),
            ("string", 2.8),
            ("function", 2.8),
            ("number", 2.8),
            ("comment", 2.2),
        ] {
            let color = r.syntax(scope).and_then(|s| s.foreground).unwrap();
            let ratio = contrast(color, background);
            assert!(ratio >= least, "{}: `{scope}` is too faint ({ratio:.1})", t.id);
        }
    }
}

/// Every bundled theme's muted text (hints, details, secondary labels) reads at WCAG AA
/// (4.5:1) on a card (the editor's background lifted a little toward the text, as the views
/// draw their cards), so a view needn't brighten it itself, and at 3:1 at least on every
/// surface it's drawn on (the window, the panels, the popups, the editor).
#[test]
fn every_builtin_theme_s_muted_text_reads_on_its_surfaces() {
    let mut registry = ThemeRegistry::default();
    for t in builtin_themes() {
        registry.register(t);
    }
    for t in registry.themes().to_vec() {
        let id = &t.id.0;
        let r = registry.resolve(&t.id).unwrap();
        let text = r.color("text").unwrap();
        let muted = r.color("text.muted").unwrap();
        let card = r.color("editor.background").unwrap().mix(text, 0.03);
        let ratio = contrast(muted, card);
        assert!(ratio >= 4.5, "{id}: the muted text reads at {ratio:.2}:1 on a card");
        for surface in
            ["window.background", "panel.background", "popup.background", "editor.background"]
        {
            let ratio = contrast(muted, r.color(surface).unwrap());
            assert!(ratio >= 3.0, "{id}: the muted text reads at {ratio:.2}:1 on `{surface}`");
        }
    }
}

/// The bundled themes' names never show the app's name (it may be renamed).
#[test]
fn builtin_theme_names_are_brand_neutral() {
    for t in builtin_themes() {
        assert!(!t.name.contains("{app}"), "{}: `{}`", t.id.0, t.name);
        assert!(
            !t.name.to_lowercase().contains(&kernel::brand::NAME.to_lowercase()),
            "{}: `{}`",
            t.id.0,
            t.name
        );
    }
}

fn resolved(theme: &Theme) -> theming::decorations::Decorations {
    theme.decorations.clone().resolve(theming::decorations::ResolveWith::default())
}

/// The first format (`[decorations.<kind>]`, still read): each kind a layer with its
/// values (palette names resolved, the rest from the kind's defaults; snow's one color its
/// list), in the order they were drawn; a theme without the section has none.
#[test]
fn the_first_format_is_still_read() {
    use theming::decorations::{
        CornerStyle, GarlandEdges, GarlandSpec, LayerSpec, Merge, ParticleShape,
    };
    let src = r##"id = "x"
name = "X"
appearance = "dark"

[palette]
red = "#aa1122"
pine = "#224433"

[decorations.garland]
edges = "top"
lights = 0.5
colors = ["$red", "#ffeeaa"]
ornaments = []
wire = "$pine"

[decorations.snow]
density = 0.2
opacity = 0.1
color = "#eeeeff"

[decorations.corners]
style = "holly"
colors = ["$red"]
"##;
    let theme = parse(src).unwrap();
    assert_eq!(theme.decorations.merge, Merge::ByKind, "each kind replaces the extended one's");
    let d = resolved(&theme);
    let [LayerSpec::Garland(g), LayerSpec::Corners(c), LayerSpec::Particles(s)] = &d.layers[..]
    else {
        panic!("{d:?}")
    };
    assert_eq!(g.edges, GarlandEdges::Top);
    assert_eq!(g.lights, 0.5);
    assert_eq!(g.colors, vec![Rgba::rgb(0xaa1122), Rgba::rgb(0xffeeaa)]);
    assert!(g.ornaments.is_empty(), "no ornaments asked: none");
    assert_eq!(g.wire, Rgba::rgb(0x224433));
    assert_eq!(g.twinkle, GarlandSpec::default().twinkle);
    assert_eq!((s.shape, s.density, s.opacity), (ParticleShape::Snow, 0.2, 0.1));
    assert_eq!(s.colors, vec![Rgba::rgb(0xeeeeff)]);
    assert_eq!(c.style, CornerStyle::Holly);
    assert_eq!(c.color, Rgba::rgb(0xaa1122));
    assert_eq!(c.accent, theming::decorations::CornerSpec::default().accent);
    // The themes that say nothing of decorations have none (and inherit them).
    let neon = BUILTIN.iter().find(|(f, _)| *f == "neon.toml").unwrap().1;
    let neon = parse(neon).unwrap();
    assert!(neon.decorations.is_empty());
    assert_eq!(neon.decorations.merge, Merge::Inherit);
}

/// The layers: each its kind and options, in order; `$name` follows the palette, `@id` a
/// setting of the theme's own, `when` a setting too; `layers = []`: none, on purpose.
#[test]
fn layers_are_read_in_order_with_what_they_follow() {
    use theming::decorations::{BindTo, Items, LayerKind, LayerSpec, Merge, ParticleShape, Strand};
    let src = r##"id = "x"
name = "X"
appearance = "light"

[palette]
leaf = "#cc6622"

[[options]]
id = "falling"
kind = "slider"
min = 0
max = 1
step = 0.1
default = 0.7

[[options]]
id = "bunting"
kind = "toggle"
default = true

[[decorations.layers]]
kind = "glow"
style = "vignette"
color = "#201008"
opacity = 0.3

[[decorations.layers]]
kind = "garland"
when = "bunting"
strand = "ribbon"
items = "flags"
colors = ["$leaf", "#ffcc00"]

[[decorations.layers]]
kind = "particles"
shape = "leaves"
density = "@falling"
colors = ["$leaf"]

[[decorations.layers]]
kind = "corners"
style = "icon"
icon = "bell"
"##;
    let theme = parse(src).unwrap();
    assert!(theme.notes.is_empty(), "{:?}", theme.notes);
    assert_eq!(theme.decorations.merge, Merge::Replace);
    let kinds: Vec<LayerKind> = theme.decorations.layers.iter().map(|l| l.spec.kind()).collect();
    assert_eq!(
        kinds,
        [LayerKind::Glow, LayerKind::Garland, LayerKind::Particles, LayerKind::Corners]
    );
    let garland = &theme.decorations.layers[1];
    assert_eq!(garland.when.as_ref().map(|w| w.text()), Some("bunting".to_owned()));
    assert_eq!(garland.binding("colors", 0), Some(&BindTo::Palette("leaf".into())));
    assert_eq!(garland.binding("colors", 1), None);
    let LayerSpec::Garland(g) = &garland.spec else { panic!() };
    assert_eq!((g.strand, g.items), (Strand::Ribbon, Items::Flags));
    let leaves = &theme.decorations.layers[2];
    assert_eq!(leaves.binding("density", 0), Some(&BindTo::Option("falling".into())));
    let LayerSpec::Particles(p) = &leaves.spec else { panic!() };
    assert_eq!((p.shape, p.density), (ParticleShape::Leaves, 0.7), "the setting's default");
    // Resolved with the user's values: the density follows, the garland hides.
    let mut registry = theming::ThemeRegistry::default();
    registry.register(theme);
    let id = theming::ThemeId::parse("x");
    let saved =
        [("falling".to_owned(), "0.3".to_owned()), ("bunting".to_owned(), "false".to_owned())];
    let d = registry.resolve_with(&id, &saved).unwrap();
    let kinds: Vec<LayerKind> = d.decorations().kinds().collect();
    assert_eq!(kinds, [LayerKind::Glow, LayerKind::Particles, LayerKind::Corners]);
    let Some(LayerSpec::Particles(p)) = d.decorations().first(LayerKind::Particles) else {
        panic!()
    };
    assert_eq!(p.density, 0.3);
    assert!(!d.decorations().dark);
    // None, on purpose.
    let none =
        parse("id = \"n\"\nname = \"N\"\nappearance = \"dark\"\n[decorations]\nlayers = []\n")
            .unwrap();
    assert!(none.decorations.is_empty());
    assert_eq!(none.decorations.merge, Merge::Replace);
}

/// The same decorations in both formats read the same.
#[test]
fn both_formats_read_the_same() {
    let head = "id = \"x\"\nname = \"X\"\nappearance = \"dark\"\n[palette]\nred = \"#aa1122\"\n";
    let old = parse(&format!(
        "{head}[decorations.garland]\nedges = \"top\"\ncolors = [\"$red\"]\n[decorations.snow]\ndensity = 0.4\n"
    ))
    .unwrap();
    let new = parse(&format!(
        "{head}[[decorations.layers]]\nkind = \"garland\"\nedges = \"top\"\ncolors = [\"$red\"]\n[[decorations.layers]]\nkind = \"particles\"\ndensity = 0.4\n"
    ))
    .unwrap();
    assert_eq!(resolved(&old), resolved(&new));
}

/// What a decoration can't be is said with its key, and the theme isn't read.
#[test]
fn wrong_decorations_are_refused() {
    let head = "id = \"x\"\nname = \"X\"\nappearance = \"dark\"\n";
    let wrong = |body: &str| parse(&format!("{head}{body}"));
    for (body, key) in [
        ("[decorations.garland]\nlights = 1.5\n", "garland.lights"),
        ("[decorations.garland]\nedges = \"bottom\"\n", "garland.edges"),
        ("[decorations.garland]\ndrop = 0.95\n", "garland.drop"),
        ("[decorations.garland]\ncolors = []\n", "garland.colors"),
        (
            "[decorations.garland]\nornaments = [\"#fff\", \"#fff\", \"#fff\", \"#fff\", \"#fff\"]\n",
            "garland.ornaments",
        ),
        ("[decorations.snow]\nopacity = 0.8\n", "snow.opacity"),
        ("[decorations.snow]\nspeed = -0.1\n", "snow.speed"),
        ("[decorations.corners]\nstyle = \"wreath\"\n", "corners.style"),
        ("[[decorations.layers]]\nkind = \"comet\"\n", "layers[0].kind"),
        ("[[decorations.layers]]\nkind = \"glow\"\nopacity = 0.9\n", "layers[0].opacity"),
        ("[[decorations.layers]]\nkind = \"particles\"\nshape = \"bats\"\n", "layers[0].shape"),
        ("[[decorations.layers]]\nkind = \"glow\"\nwhen = \"a b\"\n", "layers[0].when"),
        ("[[decorations.layers]]\nkind = \"corners\"\nicon = \"../x\"\n", "layers[0].icon"),
    ] {
        match wrong(body) {
            Err(ThemeFileError::Decoration { key: k, .. }) => assert_eq!(k, key, "{body}"),
            other => panic!("{body}: {other:?}"),
        }
    }
    assert!(matches!(
        wrong("[decorations.snow]\ncolor = \"$nope\"\n"),
        Err(ThemeFileError::Color { .. })
    ));
    // An unknown kind or key: the file is wrong.
    assert!(matches!(wrong("[decorations.confetti]\n"), Err(ThemeFileError::Syntax(_))));
    assert!(matches!(wrong("[decorations.snow]\nflakes = 3\n"), Err(ThemeFileError::Syntax(_))));
    assert!(matches!(
        wrong("[[decorations.layers]]\nkind = \"glow\"\npulse = 1\n"),
        Err(ThemeFileError::Syntax(_))
    ));
    assert!(
        matches!(
            wrong("[decorations.snow]\n[[decorations.layers]]\nkind = \"glow\"\n"),
            Err(ThemeFileError::Syntax(_)),
        ),
        "not both formats"
    );
    let many = "[[decorations.layers]]\nkind = \"glow\"\n".repeat(9);
    assert!(matches!(wrong(&many), Err(ThemeFileError::Decoration { .. })));
}

/// A theme's own settings: each kind read with its labels per language; one that can't be
/// is left out with a note (the theme is still read), as is an unknown key; a layer asking
/// for a setting the theme doesn't have keeps its value, noted.
#[test]
fn a_theme_s_own_settings_are_read_and_the_wrong_ones_noted() {
    use theming::options::{OptionKind, OptionValue};
    let src = r##"id = "x"
name = "X"
appearance = "dark"

[palette]
amber = "#f0b860"
accent = "#3574f0"

[ui]
"accent" = "$accent"

[[options]]
id = "lights"
kind = "choice"
label.en = "Lights"
label.ro = "Luminițe"
default = "cold"
shimmer = 3

[[options.choices]]
id = "warm"
label.en = "Warm"
palette.amber = "#ffaa00"

[[options.choices]]
id = "cold"
label = "Cold"
palette.amber = "#88ccff"
palette.nothing = "#000000"

[[options]]
id = "tint"
kind = "color"
palette = "accent"

[[options]]
id = "broken"
kind = "slider"
min = 1
max = 0

[[options]]
id = "mystery"
kind = "dial"

[[options]]
kind = "toggle"

[[options]]
id = "lights"
kind = "toggle"

[[decorations.layers]]
kind = "particles"
density = "@nothing"
when = "absent"
"##;
    let theme = parse(src).unwrap();
    let ids: Vec<&str> = theme.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["lights", "tint"]);
    let lights = &theme.options[0];
    assert_eq!(lights.label.get("ro"), "Luminițe");
    assert_eq!(lights.default, OptionValue::Choice("cold".into()));
    let OptionKind::Choice(choices) = &lights.kind else { panic!() };
    assert_eq!(choices[1].label.get("ro"), "Cold");
    assert_eq!(choices[1].palette, vec![("amber".to_owned(), Rgba::rgb(0x88ccff))]);
    assert_eq!(theme.options[1].default, OptionValue::Color(Rgba::rgb(0x3574f0)), "the palette's");
    assert_eq!(theme.ui_refs.get("accent").map(String::as_str), Some("accent"));
    let notes = theme.notes.join("\n");
    for expected in [
        "shimmer",
        "nothing",
        "broken",
        "dial",
        "no valid `id`",
        "declared twice",
        "absent",
        "@nothing",
    ]
    .map(|e| e.trim_start_matches('@'))
    {
        assert!(notes.contains(expected), "{expected}: {notes}");
    }
    // The color setting changes the token written with its palette name.
    let mut registry = theming::ThemeRegistry::default();
    registry.register(theme);
    let r = registry
        .resolve_with(&theming::ThemeId::parse("x"), &[("tint".into(), "#ff0000".into())])
        .unwrap();
    assert_eq!(r.color("accent"), Some(Rgba::rgb(0xff0000)));
}

/// The Christmas themes: both variants have the whole of their decorations, subtle (the
/// snow faint); Matrix its digital rain and Cyberpunk its neon (plan 45); the rest of the
/// bundled themes have none.
#[test]
fn the_christmas_themes_are_decorated_and_the_others_are_not() {
    use theming::decorations::{LayerKind, LayerSpec};
    for (file, src) in BUILTIN {
        let theme = parse(src).unwrap();
        assert!(theme.notes.is_empty(), "{file}: {:?}", theme.notes);
        let d = resolved(&theme);
        if file.starts_with("christmas-") {
            let Some(LayerSpec::Garland(g)) = d.first(LayerKind::Garland) else {
                panic!("{file}: a garland")
            };
            assert!(!g.ornaments.is_empty() && g.colors.len() >= 3, "{file}");
            assert!(g.twinkle > 0.0 && g.twinkle <= 0.5, "{file}: twinkling slowly");
            let Some(LayerSpec::Particles(snow)) = d.first(LayerKind::Particles) else {
                panic!("{file}: snow")
            };
            assert!(snow.opacity <= 0.35, "{file}: the snow stays faint");
            assert!(d.first(LayerKind::Corners).is_some(), "{file}");
        } else if *file == "matrix.toml" {
            assert_eq!(d.kinds().collect::<Vec<_>>(), [LayerKind::Matrix], "{file}");
        } else if *file == "cyberpunk.toml" {
            assert!(d.first(LayerKind::Neon).is_some(), "{file}");
            assert!(d.first(LayerKind::Synthwave).is_some(), "{file}");
        } else {
            assert!(d.is_empty(), "{file} has no decorations");
        }
    }
    let ids: Vec<String> = BUILTIN.iter().map(|(_, s)| parse(s).unwrap().id.0).collect();
    assert!(ids.contains(&"christmas-dark".to_owned()));
    assert!(ids.contains(&"christmas-light".to_owned()));
}

/// A theme made in the configurator from a Christmas theme keeps its decorations and its
/// own settings exactly (what follows the palette or a setting still does), and one made
/// from a plain theme gets the decorations chosen.
#[test]
fn themes_made_in_the_configurator_keep_the_decorations() {
    use theming::decorations::{DecorationsSpec, LayerKind, LayerSpec, ParticleSpec, presets};
    use theming::studio::{StudioTheme, compose};
    for file in ["christmas-dark.toml", "christmas-light.toml", "matrix.toml", "cyberpunk.toml"] {
        let src = BUILTIN.iter().find(|(f, _)| *f == file).unwrap().1;
        let theme = parse(src).unwrap();
        let studio = StudioTheme {
            decorations: theme.decorations.clone(),
            options: theme.options.clone(),
            ..StudioTheme::plain(&theme.id.0, &theme.name, src)
        };
        let mut draft = studio.draft().unwrap();
        draft.id = "custom-x".into();
        let made = parse(&compose(src, &draft).unwrap()).unwrap();
        assert_eq!(made.decorations, theme.decorations, "{file}");
        assert_eq!(made.options, theme.options, "{file}");
        assert!(made.notes.is_empty(), "{file}: {:?}", made.notes);
    }
    let neon = BUILTIN.iter().find(|(f, _)| *f == "neon.toml").unwrap().1;
    let mut draft = StudioTheme::plain("neon", "Neon", neon).draft().unwrap();
    draft.id = "custom-festive".into();
    draft.decorations = DecorationsSpec::of(presets::layers("autumn", true).unwrap());
    draft.decorations.layers.push(theming::decorations::DecorationLayer::plain(
        LayerSpec::Particles(ParticleSpec { density: 0.6, ..ParticleSpec::default() }),
    ));
    let made = resolved(&parse(&compose(neon, &draft).unwrap()).unwrap());
    let mut expected = presets::layers("autumn", true).unwrap();
    expected.push(LayerSpec::Particles(ParticleSpec { density: 0.6, ..ParticleSpec::default() }));
    assert_eq!(made.layers, expected);
    assert_eq!(made.kinds().filter(|k| *k == LayerKind::Particles).count(), 2);
}

/// The Christmas themes' own settings: the lights' colors (a choice changing the palette
/// the garland follows), the snow (a slider), the bows (a toggle hiding the corners); at
/// their defaults, the decorations they always had.
#[test]
fn the_christmas_settings_drive_its_decorations() {
    use theming::decorations::{LayerKind, LayerSpec};
    let mut registry = theming::ThemeRegistry::default();
    for theme in builtin_themes() {
        registry.register(theme);
    }
    for id in ["christmas-dark", "christmas-light"] {
        let id = theming::ThemeId::parse(id);
        let plain = registry.resolve(&id).unwrap();
        let ids: Vec<&str> = plain.options().iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["lights", "snow", "bows"]);
        let garland = |r: &theming::ResolvedTheme| match r.decorations().first(LayerKind::Garland) {
            Some(LayerSpec::Garland(g)) => g.colors.clone(),
            _ => panic!(),
        };
        let snow = |r: &theming::ResolvedTheme| match r.decorations().first(LayerKind::Particles) {
            Some(LayerSpec::Particles(p)) => p.density,
            _ => panic!(),
        };
        assert_eq!(snow(&plain), 0.5);
        assert!(plain.decorations().first(LayerKind::Corners).is_some());
        let saved = [
            ("lights".to_owned(), "icy".to_owned()),
            ("snow".to_owned(), "0.2".to_owned()),
            ("bows".to_owned(), "false".to_owned()),
        ];
        let chosen = registry.resolve_with(&id, &saved).unwrap();
        assert_ne!(garland(&chosen), garland(&plain), "{id}: the lights' colors follow");
        assert!((snow(&chosen) - 0.2).abs() < 1e-6);
        assert!(chosen.decorations().first(LayerKind::Corners).is_none(), "no bows");
        // The rest of the theme stays.
        assert_eq!(chosen.color("accent"), plain.color("accent"));
    }
}

/// A layer switched off (the studio's switch): kept with its options through a save and a
/// read, but not drawn; `enabled` takes only true or false.
#[test]
fn a_layer_switched_off_is_kept_but_not_drawn() {
    use theming::decorations::{DecorationsSpec, LayerKind, LayerSpec};
    use theming::studio::{StudioTheme, compose};
    let neon = BUILTIN.iter().find(|(f, _)| *f == "neon.toml").unwrap().1;
    let mut draft = StudioTheme::plain("neon", "Neon", neon).draft().unwrap();
    draft.id = "custom-off".into();
    draft.decorations =
        DecorationsSpec::of([LayerSpec::new(LayerKind::Glow), LayerSpec::new(LayerKind::Corners)]);
    draft.decorations.layers[0].enabled = false;
    let src = compose(neon, &draft).unwrap();
    assert!(src.contains("kind = \"glow\"\nenabled = false"), "{src}");
    let theme = parse(&src).unwrap();
    assert!(!theme.decorations.layers[0].enabled && theme.decorations.layers[1].enabled);
    assert_eq!(resolved(&theme).kinds().collect::<Vec<_>>(), [LayerKind::Corners]);
    let wrong = src.replace("enabled = false", "enabled = \"no\"");
    assert!(parse(&wrong).is_err());
}

/// Every bundled theme composed by the theme studio as it is (nothing edited: effects added,
/// a name) keeps its UI colors exactly — none taken for its accent (Matrix's near-black
/// greens once turned the side panels bright green).
#[test]
fn a_bundled_theme_composed_unchanged_keeps_its_ui_colors() {
    use theming::studio::{compose, read_draft};
    let ui = |src: &str| -> Vec<String> {
        let rest = src.split_once("\n[ui]\n").map_or("", |(_, r)| r);
        let ui = rest.split("\n[").next().unwrap_or("");
        ui.lines()
            .filter(|l| l.contains('#') && !l.trim_start().starts_with('#'))
            .map(|l| l.split_whitespace().collect::<String>().to_lowercase())
            .collect()
    };
    for (name, src) in BUILTIN {
        let Some(mut draft) = read_draft(src) else { continue };
        draft.id = "mine".into();
        let out = compose(src, &draft).unwrap_or_else(|| panic!("{name} composed"));
        let after = ui(&out);
        for line in ui(src) {
            assert!(after.contains(&line), "{name}: {line}");
        }
    }
}

/// Matrix comes with its digital rain on (plan 45): faint, behind the panels (the editor
/// stays clean), Katakana and digits in the theme's green; the user's choice saved for the
/// theme wins over its default — off hides the rain, a density thins it —, and no other
/// theme has it.
#[test]
fn matrix_rains_by_default_and_the_user_s_choice_wins() {
    use theming::decorations::{LayerKind, LayerSpec, Zone};
    let mut registry = theming::ThemeRegistry::default();
    for theme in builtin_themes() {
        registry.register(theme);
    }
    let matrix = theming::ThemeId::parse("matrix");
    let plain = registry.resolve(&matrix).unwrap();
    let ids: Vec<&str> = plain.options().iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["rain", "density"]);
    let rain = |r: &theming::ResolvedTheme| match r.decorations().first(LayerKind::Matrix) {
        Some(LayerSpec::Effect(e)) => Some(e.clone()),
        _ => None,
    };
    let e = rain(&plain).expect("the rain, on by default");
    assert_eq!(e.style, "katakana");
    assert_eq!(e.zone, Zone::Panels, "behind the panels, never the editor");
    assert!(e.opacity <= 0.25 && e.glass <= 0.4, "faint: {} / {}", e.opacity, e.glass);
    assert_eq!(e.colors, vec![plain.color("accent").unwrap()], "the theme's green");
    assert!((e.density - 0.4).abs() < 1e-6);
    // Turned off by the user: gone, the rest of the theme as it was.
    let off = registry.resolve_with(&matrix, &[("rain".into(), "false".into())]).unwrap();
    assert!(rain(&off).is_none() && off.decorations().is_empty());
    assert_eq!(off.color("accent"), plain.color("accent"));
    // Thinned: the density chosen.
    let thin = registry.resolve_with(&matrix, &[("density".into(), "0.1".into())]).unwrap();
    assert!((rain(&thin).unwrap().density - 0.1).abs() < 1e-6);
    // Another theme: no rain (it's Matrix's own).
    for id in ["default-dark", "midnight", "neon"] {
        let other = registry.resolve(&theming::ThemeId::parse(id)).unwrap();
        assert!(other.decorations().first(LayerKind::Matrix).is_none(), "{id}");
    }
}

/// The Cyberpunk theme (plan 45): violet-black, magenta and cyan, electric yellow warnings;
/// its decorations on by default and faint, each with its switch in the theme's settings.
#[test]
fn cyberpunk_is_neon_on_violet_black() {
    use theming::decorations::{LayerKind, LayerSpec};
    let mut registry = theming::ThemeRegistry::default();
    for theme in builtin_themes() {
        registry.register(theme);
    }
    let id = theming::ThemeId::parse("cyberpunk");
    let r = registry.resolve(&id).unwrap();
    assert_eq!(r.appearance, theming::Appearance::Dark);
    let bg = r.color("editor.background").unwrap();
    assert!(bg.b > bg.g && bg.r > bg.g && bg.b < 0.15, "violet-black: {bg:?}");
    let accent = r.color("accent").unwrap();
    assert!(accent.r > 0.9 && accent.b > 0.7 && accent.g < 0.3, "magenta: {accent:?}");
    let warning = r.color("warning").unwrap();
    assert!(warning.r > 0.9 && warning.g > 0.85 && warning.b < 0.2, "yellow: {warning:?}");
    let string = r.syntax("string").and_then(|s| s.foreground).unwrap();
    assert!(string.g > 0.8 && string.b > 0.9 && string.r < 0.2, "cyan: {string:?}");
    for token in ["terminal.ansi.magenta", "terminal.ansi.brightWhite", "icon.php", "icon.file"] {
        assert!(r.color(token).is_some(), "{token}");
    }
    for layer in &r.decorations().layers {
        let LayerSpec::Effect(e) = layer else { panic!("{layer:?}") };
        assert!(e.opacity <= 0.45, "{:?} faint: {}", e.kind, e.opacity);
    }
    let kinds: Vec<LayerKind> = r.decorations().kinds().collect();
    assert_eq!(
        kinds,
        [LayerKind::Neon, LayerKind::Synthwave, LayerKind::NeonRain, LayerKind::Glitch],
        "the signs off until switched on"
    );
    let signs = registry.resolve_with(&id, &[("signs".into(), "true".into())]).unwrap();
    assert!(signs.decorations().first(LayerKind::HoloSigns).is_some());
    let off: Vec<(String, String)> = ["neon", "grid", "rain", "glitch"]
        .iter()
        .map(|o| ((*o).to_owned(), "false".to_owned()))
        .collect();
    let off = registry.resolve_with(&id, &off).unwrap();
    assert!(off.decorations().is_empty(), "each has its switch");
}
