use super::*;
use crate::options::OptionValue;

#[test]
fn every_id_is_read_back() {
    for kind in LayerKind::ALL {
        assert_eq!(LayerKind::parse(kind.id()), Some(*kind));
    }
    for s in Strand::ALL {
        assert_eq!(Strand::parse(s.id()), Some(*s));
    }
    for i in Items::ALL {
        assert_eq!(Items::parse(i.id()), Some(*i));
    }
    for p in ParticleShape::ALL {
        assert_eq!(ParticleShape::parse(p.id()), Some(*p));
    }
    for c in CornerStyle::ALL {
        assert_eq!(CornerStyle::parse(c.id()), Some(*c));
    }
    for g in GlowStyle::ALL {
        assert_eq!(GlowStyle::parse(g.id()), Some(*g));
    }
    assert_eq!(LayerKind::parse("comet"), None);
    assert_eq!(GarlandEdges::parse("bottom"), None);
    assert_eq!(Items::parse("none"), Some(Items::Nothing));
}

/// Every option of every kind: read with `get`, taken back by `set`, the same after a trip
/// through a line of the settings.
#[test]
fn every_field_is_read_written_and_encoded() {
    for kind in LayerKind::ALL {
        let spec = LayerSpec::new(*kind);
        assert_eq!(spec.kind(), *kind);
        for f in kind.fields() {
            let value = spec.get(f.key).unwrap_or_else(|| panic!("{}.{}", kind.id(), f.key));
            let mut again = spec.clone();
            assert_eq!(again.set(f.key, &value), Ok(()), "{}.{}", kind.id(), f.key);
            assert_eq!(again, spec);
        }
        assert_eq!(decode(&encode(&spec)), Ok(spec.clone()), "{}", encode(&spec));
        assert_eq!(spec.get("nope"), None);
        assert_eq!(spec.clone().set("nope", &FieldValue::Number(0.5)), Err(FieldError::Unknown));
    }
    let wild = LayerSpec::Garland(GarlandSpec {
        strand: Strand::Ribbon,
        items: Items::Flags,
        edges: GarlandEdges::Top,
        colors: vec![Rgba::rgb(0x123456), Rgba::from_hex("#ff000080").unwrap()],
        ..GarlandSpec::default()
    });
    let line = encode(&wild);
    assert!(line.starts_with("kind=garland;strand=ribbon;items=flags;edges=top;"), "{line}");
    assert_eq!(decode(&line), Ok(wild));
}

#[test]
fn wrong_values_are_refused_with_why() {
    let mut g = LayerSpec::new(LayerKind::Garland);
    let n = FieldValue::Number;
    assert_eq!(g.set("lights", &n(1.5)), Err(FieldError::Invalid("from 0 to 1")));
    assert_eq!(g.set("drop", &n(0.95)), Err(FieldError::Invalid("out of range")));
    assert_eq!(g.set("lights", &n(f32::NAN)), Err(FieldError::Invalid("from 0 to 1")));
    assert!(matches!(
        g.set("edges", &FieldValue::Text("bottom".into())),
        Err(FieldError::Invalid(_))
    ));
    assert!(matches!(
        g.set("colors", &FieldValue::Colors(Vec::new())),
        Err(FieldError::Invalid(_))
    ));
    assert!(matches!(
        g.set("ornaments", &FieldValue::Colors(vec![Rgba::WHITE; 5])),
        Err(FieldError::Invalid(_))
    ));
    // `sides`: the first format's alias.
    assert_eq!(g.set("edges", &FieldValue::Text("sides".into())), Ok(()));
    let mut p = LayerSpec::new(LayerKind::Particles);
    assert!(matches!(p.set("opacity", &n(0.8)), Err(FieldError::Invalid(_))), "stays faint");
    // The first format's single color.
    assert_eq!(p.set("color", &FieldValue::Color(Rgba::rgb(0x112233))), Ok(()));
    assert_eq!(p.get("colors"), Some(FieldValue::Colors(vec![Rgba::rgb(0x112233)])));
    let mut c = LayerSpec::new(LayerKind::Corners);
    assert!(matches!(c.set("icon", &FieldValue::Text("a b".into())), Err(FieldError::Invalid(_))));
    let mut glow = LayerSpec::new(LayerKind::Glow);
    assert!(matches!(glow.set("opacity", &n(0.7)), Err(FieldError::Invalid(_))));
    assert!(decode("kind=garland;lights=2").is_err());
    assert!(decode("kind=comet").is_err());
    assert!(decode("strand=wire").is_err(), "no kind");
    assert!(decode("kind=glow;flicker=1").is_err());
}

#[test]
fn colors_are_set_by_place() {
    let mut g = LayerSpec::new(LayerKind::Garland);
    assert!(g.set_color_at("colors", 1, Rgba::rgb(0x00ff00)));
    assert_eq!(g.color_at("colors", 1), Some(Rgba::rgb(0x00ff00)));
    assert!(g.set_color_at("wire", 0, Rgba::rgb(0x0000ff)));
    assert_eq!(g.color_at("wire", 0), Some(Rgba::rgb(0x0000ff)));
    assert!(!g.set_color_at("colors", 9, Rgba::WHITE), "no such color");
    assert!(!g.set_color_at("lights", 0, Rgba::WHITE), "not a color");
    let mut c = LayerSpec::new(LayerKind::Corners);
    assert!(c.set_color_at("colors", 1, Rgba::rgb(0x445566)));
    let LayerSpec::Corners(c) = c else { unreachable!() };
    assert_eq!(c.accent, Rgba::rgb(0x445566));
}

#[test]
fn nothing_declared_is_empty_and_still() {
    let none = DecorationsSpec::default().resolve(ResolveWith::default());
    assert!(none.is_empty());
    assert!(!none.moves());
    assert_eq!(none.kinds().count(), 0);
}

fn layer(spec: LayerSpec) -> DecorationLayer {
    DecorationLayer::plain(spec)
}

/// The first format: each kind a theme declares replaces the extended theme's of that kind
/// (in its place), the others stay; a list of layers replaces the whole; none declared:
/// the extended theme's.
#[test]
fn themes_combine_with_what_they_extend() {
    let base = DecorationsSpec {
        layers: vec![
            layer(LayerSpec::new(LayerKind::Garland)),
            layer(LayerSpec::new(LayerKind::Particles)),
        ],
        merge: Merge::ByKind,
    };
    let legacy = DecorationsSpec {
        layers: vec![
            layer(LayerSpec::Particles(ParticleSpec { density: 0.9, ..ParticleSpec::default() })),
            layer(LayerSpec::new(LayerKind::Corners)),
        ],
        merge: Merge::ByKind,
    };
    let d = legacy.over(base.clone()).resolve(ResolveWith::default());
    let kinds: Vec<LayerKind> = d.kinds().collect();
    assert_eq!(kinds, [LayerKind::Garland, LayerKind::Particles, LayerKind::Corners]);
    let Some(LayerSpec::Particles(p)) = d.first(LayerKind::Particles) else { panic!() };
    assert_eq!(p.density, 0.9);
    let list = DecorationsSpec::of([LayerSpec::new(LayerKind::Glow)]);
    let d = list.over(base.clone()).resolve(ResolveWith::default());
    assert_eq!(d.kinds().collect::<Vec<_>>(), [LayerKind::Glow], "a list replaces the whole");
    let none = DecorationsSpec { layers: Vec::new(), merge: Merge::Replace };
    assert!(none.over(base.clone()).resolve(ResolveWith::default()).is_empty(), "`layers = []`");
    let inherit = DecorationsSpec::default().over(base.clone());
    assert_eq!(inherit, base);
}

#[test]
fn limits_are_kept() {
    let wild = DecorationsSpec::of([
        LayerSpec::Garland(GarlandSpec {
            lights: 7.0,
            twinkle: -1.0,
            drop: 1.0,
            opacity: f32::NAN,
            colors: Vec::new(),
            ornaments: vec![Rgba::WHITE; 9],
            ..GarlandSpec::default()
        }),
        LayerSpec::Particles(ParticleSpec {
            opacity: 0.9,
            speed: 3.0,
            colors: Vec::new(),
            ..ParticleSpec::default()
        }),
        LayerSpec::Corners(CornerSpec {
            opacity: 2.0,
            icon: "../x y".into(),
            ..CornerSpec::default()
        }),
        LayerSpec::Glow(GlowSpec { opacity: 1.0, width: 4.0, ..GlowSpec::default() }),
    ])
    .resolve(ResolveWith::default());
    let [
        LayerSpec::Garland(g),
        LayerSpec::Particles(p),
        LayerSpec::Corners(c),
        LayerSpec::Glow(glow),
    ] = &wild.layers[..]
    else {
        panic!("{wild:?}")
    };
    assert_eq!((g.lights, g.twinkle, g.drop, g.opacity), (1.0, 0.0, MAX_DROP, 0.0));
    assert_eq!(g.colors, GarlandSpec::default().colors, "a garland always has an item color");
    assert_eq!(g.ornaments.len(), MAX_ORNAMENT_COLORS);
    assert_eq!((p.opacity, p.speed), (MAX_SNOW_OPACITY, 1.0), "particles stay faint");
    assert!(!p.colors.is_empty());
    assert_eq!(c.opacity, 1.0);
    assert_eq!(c.icon, CornerSpec::default().icon);
    assert_eq!((glow.opacity, glow.width), (MAX_GLOW_OPACITY, 1.0));
    // At most so many layers.
    let many = DecorationsSpec::of((0..20).map(|_| LayerSpec::new(LayerKind::Glow)));
    assert_eq!(many.resolve(ResolveWith::default()).layers.len(), MAX_LAYERS);
}

#[test]
fn only_what_glints_or_drifts_moves() {
    let still = |spec: LayerSpec| !spec.moves();
    assert!(still(LayerSpec::Garland(GarlandSpec { twinkle: 0.0, ..GarlandSpec::default() })));
    assert!(still(LayerSpec::Garland(GarlandSpec {
        items: Items::Flags,
        ..GarlandSpec::default()
    })));
    assert!(still(LayerSpec::Garland(GarlandSpec { lights: 0.0, ..GarlandSpec::default() })));
    assert!(still(LayerSpec::new(LayerKind::Corners)));
    assert!(still(LayerSpec::new(LayerKind::Glow)));
    assert!(LayerSpec::new(LayerKind::Garland).moves());
    assert!(LayerSpec::new(LayerKind::Particles).moves());
    let stars = |speed| {
        LayerSpec::Particles(ParticleSpec {
            shape: ParticleShape::Stars,
            speed,
            ..ParticleSpec::default()
        })
    };
    assert!(stars(0.5).moves() && still(stars(0.0)), "stars glow only with a speed");
    let falling = LayerSpec::Particles(ParticleSpec { speed: 0.0, ..ParticleSpec::default() });
    assert!(falling.moves(), "snow falls, if slowly");
}

/// What a layer follows: the palette's names an option changes, the options' values, and
/// when it's drawn at all.
#[test]
fn layers_follow_the_palette_and_the_options() {
    let snow = DecorationLayer {
        spec: LayerSpec::new(LayerKind::Particles),
        when: Condition::parse("snow"),
        bindings: vec![
            Binding { key: "density", index: 0, to: BindTo::Option("amount".into()) },
            Binding { key: "colors", index: 0, to: BindTo::Palette("flake".into()) },
        ],
        enabled: true,
    };
    let warm = DecorationLayer {
        spec: LayerSpec::new(LayerKind::Glow),
        when: Condition::parse("lights=warm"),
        bindings: vec![Binding { key: "color", index: 0, to: BindTo::Option("tint".into()) }],
        enabled: true,
    };
    let not_cold = DecorationLayer {
        when: Condition::parse("!cold"),
        ..layer(LayerSpec::new(LayerKind::Corners))
    };
    let unknown = DecorationLayer {
        when: Condition::parse("nothing"),
        ..layer(LayerSpec::new(LayerKind::Garland))
    };
    let spec =
        DecorationsSpec { layers: vec![snow, warm, not_cold, unknown], merge: Merge::Replace };
    let values = vec![
        ("snow".to_owned(), OptionValue::Bool(true)),
        ("amount".to_owned(), OptionValue::Number(0.8)),
        ("lights".to_owned(), OptionValue::Choice("warm".into())),
        ("tint".to_owned(), OptionValue::Color(Rgba::rgb(0x00ff00))),
        ("cold".to_owned(), OptionValue::Bool(false)),
    ];
    let palette = vec![("flake".to_owned(), Rgba::rgb(0xaabbcc))];
    let d = spec.clone().resolve(ResolveWith { values: &values, palette: &palette, dark: true });
    assert!(d.dark);
    let kinds: Vec<LayerKind> = d.kinds().collect();
    assert_eq!(
        kinds,
        [LayerKind::Particles, LayerKind::Glow, LayerKind::Corners, LayerKind::Garland]
    );
    let Some(LayerSpec::Particles(p)) = d.first(LayerKind::Particles) else { panic!() };
    assert_eq!((p.density, p.colors[0]), (0.8, Rgba::rgb(0xaabbcc)));
    let Some(LayerSpec::Glow(g)) = d.first(LayerKind::Glow) else { panic!() };
    assert_eq!(g.color, Rgba::rgb(0x00ff00));
    // Turned off, another choice, the negation holding no more.
    let values = vec![
        ("snow".to_owned(), OptionValue::Bool(false)),
        ("lights".to_owned(), OptionValue::Choice("cold".into())),
        ("cold".to_owned(), OptionValue::Bool(true)),
    ];
    let d = spec.resolve(ResolveWith { values: &values, palette: &[], dark: false });
    assert_eq!(d.kinds().collect::<Vec<_>>(), [LayerKind::Garland], "only the unconditioned one");
}

#[test]
fn conditions_are_read_and_written() {
    for text in ["snow", "!snow", "lights=warm", "!lights=cold"] {
        assert_eq!(Condition::parse(text).unwrap().text(), text);
    }
    assert_eq!(Condition::parse(" ! snow ").unwrap().text(), "!snow");
    assert!(Condition::parse("").is_none() && Condition::parse("a b").is_none());
    assert!(Condition::parse("x=").is_none());
}

#[test]
fn the_user_s_layers_go_over_or_instead() {
    let theme = Decorations { layers: vec![LayerSpec::new(LayerKind::Garland)], dark: true };
    let mine = [LayerSpec::new(LayerKind::Glow)];
    let added = theme.with_overlay(&mine, OverlayMode::Add);
    assert_eq!(added.kinds().collect::<Vec<_>>(), [LayerKind::Garland, LayerKind::Glow]);
    assert!(added.dark);
    let instead = theme.with_overlay(&mine, OverlayMode::Replace);
    assert_eq!(instead.kinds().collect::<Vec<_>>(), [LayerKind::Glow]);
    for mode in OverlayMode::ALL {
        assert_eq!(OverlayMode::parse(mode.id()), Some(mode));
    }
}

#[test]
fn the_registry_resolves_decorations_through_extends() {
    use crate::{Appearance, Theme, ThemeRegistry};
    let theme = |id: &str, extends: Option<&str>, decorations: DecorationsSpec| Theme {
        extends: extends.map(crate::ThemeId::parse),
        decorations,
        ..Theme::new(id, id, Appearance::Dark)
    };
    let legacy =
        |spec: LayerSpec| DecorationsSpec { layers: vec![layer(spec)], merge: Merge::ByKind };
    let mut registry = ThemeRegistry::default();
    registry.register(theme("base", None, legacy(LayerSpec::new(LayerKind::Garland))));
    registry.register(theme("child", Some("base"), legacy(LayerSpec::new(LayerKind::Particles))));
    registry.register(theme("plain", None, DecorationsSpec::default()));
    let child = registry.resolve(&crate::ThemeId::parse("child")).unwrap();
    let kinds: Vec<LayerKind> = child.decorations().kinds().collect();
    assert_eq!(kinds, [LayerKind::Garland, LayerKind::Particles]);
    assert!(child.decorations().dark);
    let plain = registry.resolve(&crate::ThemeId::parse("plain")).unwrap();
    assert!(plain.decorations().is_empty());
    // What the studio edits: the chain's layers, not resolved.
    let (declared, options) = registry.declared(&crate::ThemeId::parse("child"), &[]).unwrap();
    assert_eq!(declared.layers.len(), 2);
    assert!(options.is_empty());
}
