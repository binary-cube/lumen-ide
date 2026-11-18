use super::*;

fn choice(id: &str, palette: &[(&str, u32)]) -> OptionChoice {
    OptionChoice {
        id: id.into(),
        label: Label::plain(id),
        palette: palette.iter().map(|(n, c)| ((*n).to_owned(), Rgba::rgb(*c))).collect(),
    }
}

fn lights() -> ThemeOption {
    ThemeOption {
        id: "lights".into(),
        label: Label(vec![("en".into(), "Lights".into()), ("ro".into(), "Luminițe".into())]),
        kind: OptionKind::Choice(vec![
            choice("warm", &[("amber", 0xf0b860)]),
            choice("cold", &[("amber", 0x8fd0ff), ("ivory", 0xe0f0ff)]),
        ]),
        default: OptionValue::Choice("warm".into()),
    }
}

fn snow() -> ThemeOption {
    ThemeOption {
        id: "snow".into(),
        label: Label::plain("Snow"),
        kind: OptionKind::Slider { min: 0.0, max: 0.8, step: 0.1 },
        default: OptionValue::Number(0.5),
    }
}

#[test]
fn labels_follow_the_language() {
    let l = lights().label;
    assert_eq!(l.get("ro"), "Luminițe");
    assert_eq!(l.get("ro-RO"), "Luminițe", "the language without its region");
    assert_eq!(l.get("fr"), "Lights", "else English");
    assert_eq!(Label(vec![("de".into(), "Licht".into())]).get("ro"), "Licht", "else the first");
    assert_eq!(Label::default().get("en"), "");
}

#[test]
fn values_are_taken_only_when_valid() {
    let (lights, snow) = (lights(), snow());
    assert_eq!(lights.parse_value("cold"), Some(OptionValue::Choice("cold".into())));
    assert_eq!(lights.parse_value("blinking"), None);
    assert_eq!(snow.parse_value("0.3"), Some(OptionValue::Number(0.3)));
    assert_eq!(snow.parse_value("2"), None, "out of range");
    assert_eq!(snow.parse_value("much"), None);
    let toggle = ThemeOption {
        id: "bows".into(),
        label: Label::plain("Bows"),
        kind: OptionKind::Toggle,
        default: OptionValue::Bool(true),
    };
    assert_eq!(toggle.parse_value("false"), Some(OptionValue::Bool(false)));
    assert_eq!(toggle.parse_value("maybe"), None);
    let color = ThemeOption {
        id: "tint".into(),
        label: Label::plain("Tint"),
        kind: OptionKind::Color { palette: Some("accent".into()) },
        default: OptionValue::Color(Rgba::WHITE),
    };
    assert_eq!(color.parse_value("#ff0000"), Some(OptionValue::Color(Rgba::rgb(0xff0000))));
    assert_eq!(color.parse_value("red"), None);
    assert_eq!(
        color.palette_of(&OptionValue::Color(Rgba::BLACK)),
        vec![("accent".to_owned(), Rgba::BLACK)]
    );
}

#[test]
fn a_slider_has_its_stops() {
    assert_eq!(snow().stops().len(), 9);
    assert_eq!(snow().stops()[0], 0.0);
    assert!((snow().stops()[8] - 0.8).abs() < 1e-4);
    let fine =
        ThemeOption { kind: OptionKind::Slider { min: 0.0, max: 100.0, step: 0.5 }, ..snow() };
    assert!(fine.stops().len() <= MAX_STOPS + 1, "the step widened");
    let odd = ThemeOption { kind: OptionKind::Slider { min: 0.0, max: 1.0, step: 0.3 }, ..snow() };
    assert_eq!(*odd.stops().last().unwrap(), 1.0, "the maximum is a stop");
    assert!(lights().stops().is_empty());
}

#[test]
fn effective_values_and_the_palette_they_change() {
    let options = [lights(), snow()];
    let saved = vec![
        ("lights".to_owned(), "cold".to_owned()),
        ("snow".to_owned(), "9".to_owned()),
        ("gone".to_owned(), "x".to_owned()),
    ];
    let values = effective(&options, &saved);
    assert_eq!(
        values,
        vec![
            ("lights".to_owned(), OptionValue::Choice("cold".into())),
            ("snow".to_owned(), OptionValue::Number(0.5)),
        ],
        "an invalid value: the default; an option that doesn't exist: left out"
    );
    let palette = palette_overrides(&options, &values);
    assert_eq!(palette.len(), 2);
    assert!(palette.contains(&("amber".to_owned(), Rgba::rgb(0x8fd0ff))));
}

#[test]
fn a_theme_s_options_replace_the_extended_ones_by_id() {
    let mut mine = snow();
    mine.default = OptionValue::Number(0.2);
    let merged = over(&[mine.clone()], &[lights(), snow()]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1], mine, "in its place");
    let added = over(&[ThemeOption { id: "x".into(), ..snow() }], &[lights()]);
    assert_eq!(added.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["lights", "x"]);
}

#[test]
fn the_settings_keep_every_theme_s_values_in_one_list() {
    let list = with_saved(&[], "christmas-dark", "lights", &OptionValue::Choice("cold".into()));
    let list = with_saved(&list, "christmas-dark", "snow", &OptionValue::Number(0.3));
    let list = with_saved(&list, "other", "snow", &OptionValue::Number(0.7));
    let list = with_saved(&list, "christmas-dark", "lights", &OptionValue::Choice("warm".into()));
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(
        saved_for(&list, "christmas-dark"),
        vec![("snow".to_owned(), "0.3".to_owned()), ("lights".to_owned(), "warm".to_owned())]
    );
    assert!(list.iter().all(|e| valid_entry(e)));
    assert!(!valid_entry("no-slash") && !valid_entry("t/=x") && !valid_entry("/a=b"));
    assert!(saved_for(&["broken".to_owned()], "t").is_empty());
}
