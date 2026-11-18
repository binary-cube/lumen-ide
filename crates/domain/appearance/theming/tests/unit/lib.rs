use super::*;

fn theme(id: &str, appearance: Appearance, extends: Option<&str>, ui: &[(&str, u32)]) -> Theme {
    Theme {
        extends: extends.map(|e| ThemeId(e.into())),
        ui: ui.iter().map(|(k, c)| ((*k).to_owned(), Rgba::rgb(*c))).collect(),
        ..Theme::new(id, id, appearance)
    }
}

#[test]
fn hierarchical_lookup() {
    let mut r = ThemeRegistry::default();
    r.register(theme(
        "t",
        Appearance::Dark,
        None,
        &[("editor", 0x111111), ("editor.gutter", 0x222222)],
    ));
    let t = r.resolve(&ThemeId("t".into())).unwrap();
    assert_eq!(t.color("editor.gutter.foreground.active"), Some(Rgba::rgb(0x222222)));
    assert_eq!(t.color("editor.background"), Some(Rgba::rgb(0x111111)));
    assert_eq!(t.color("tree.selection"), None);
}

#[test]
fn extends_overrides_base() {
    let mut r = ThemeRegistry::default();
    r.register(theme("base", Appearance::Dark, None, &[("accent", 0x0000ff), ("text", 0xffffff)]));
    r.register(theme("child", Appearance::Dark, Some("base"), &[("accent", 0xff0000)]));
    let t = r.resolve(&ThemeId("child".into())).unwrap();
    assert_eq!(t.color("accent"), Some(Rgba::rgb(0xff0000)));
    assert_eq!(t.color("text"), Some(Rgba::rgb(0xffffff)));
}

#[test]
fn missing_and_circular_themes_are_errors() {
    let mut r = ThemeRegistry::default();
    r.register(theme("a", Appearance::Dark, Some("b"), &[]));
    r.register(theme("b", Appearance::Dark, Some("a"), &[]));
    assert!(matches!(r.resolve(&ThemeId("a".into())), Err(ThemeError::CircularExtends(_))));
    assert!(matches!(r.resolve(&ThemeId("x".into())), Err(ThemeError::NotFound(_))));
}

#[test]
fn registry_navigation_and_override() {
    let mut r = ThemeRegistry::default();
    r.register(theme("dark", Appearance::Dark, None, &[]));
    r.register(theme("light", Appearance::Light, None, &[]));
    r.register(theme("dark", Appearance::Dark, None, &[("accent", 1)]));
    assert_eq!(r.themes().len(), 2);
    assert_eq!(r.first_with(Appearance::Light), Some(&ThemeId("light".into())));
    assert_eq!(r.next_after(&ThemeId("light".into())), Some(&ThemeId("dark".into())));
}

/// The caret: what a theme says over what it extends, the rest as always.
#[test]
fn the_caret_comes_through_the_themes_it_extends() {
    use caret::{Blink, CaretShape, CaretSpec, Motion};
    let mut r = ThemeRegistry::default();
    let mut base = theme("base", Appearance::Dark, None, &[]);
    base.caret = CaretSpec { glow: Some(0.6), blink: Some(Blink::Smooth), ..CaretSpec::default() };
    let mut top = theme("top", Appearance::Dark, Some("base"), &[]);
    top.caret =
        CaretSpec { shape: Some(CaretShape::Block), glow: Some(0.2), ..CaretSpec::default() };
    r.register(base);
    r.register(top);
    let look = r.resolve(&ThemeId("top".into())).unwrap().caret();
    assert_eq!(
        (look.shape, look.blink, look.motion),
        (CaretShape::Block, Blink::Smooth, Motion::Jump)
    );
    assert!((look.glow - 0.2).abs() < f32::EPSILON);
    r.register(theme("plain", Appearance::Dark, None, &[]));
    assert_eq!(r.resolve(&ThemeId("plain".into())).unwrap().caret(), caret::CaretLook::default());
}

/// The ids saved before the bundled themes were renamed still name them (`lumen-dark` was
/// left as it was: the alias stripped an empty prefix); today's ids and the user's are as
/// written.
#[test]
fn ids_saved_before_the_rename_name_today_s_themes() {
    assert_eq!(ThemeId::parse("lumen-dark").0, "default-dark");
    assert_eq!(ThemeId::parse("lumen-light").0, "default-light");
    assert_eq!(ThemeId::parse("lumen-arctic").0, "arctic");
    assert_eq!(ThemeId::parse("default-dark").0, "default-dark");
    assert_eq!(ThemeId::parse("dark").0, "dark", "a user's theme named `dark` is its own");
}

/// A theme's own settings: a choice changes the palette's names the tokens were written
/// with (the interface and the syntax follow), the user's value over the default; a child
/// theme replaces an option by id; what the studio is given has the user's values as the
/// defaults.
#[test]
fn a_theme_s_options_change_the_tokens_written_with_their_names() {
    use options::{Label, OptionChoice, OptionKind, OptionValue, ThemeOption};
    let mut base =
        theme("base", Appearance::Dark, None, &[("accent", 0x0000ff), ("text", 0xffffff)]);
    base.ui_refs.insert("accent".into(), "accent".into());
    base.syntax.insert(
        "string".into(),
        TextStyle { foreground: Some(Rgba::rgb(0x00ff00)), italic: true, ..TextStyle::default() },
    );
    base.syntax_refs.insert("string".into(), "green".into());
    let choice = |id: &str, palette: &[(&str, u32)]| OptionChoice {
        id: id.into(),
        label: Label::plain(id),
        palette: palette.iter().map(|(n, c)| ((*n).to_owned(), Rgba::rgb(*c))).collect(),
    };
    base.options.push(ThemeOption {
        id: "tone".into(),
        label: Label::plain("Tone"),
        kind: OptionKind::Choice(vec![
            choice("cool", &[]),
            choice("warm", &[("accent", 0xff8800), ("green", 0x88aa00)]),
        ]),
        default: OptionValue::Choice("cool".into()),
    });
    let mut r = ThemeRegistry::default();
    r.register(base);
    let id = ThemeId("base".into());
    let plain = r.resolve(&id).unwrap();
    assert_eq!(plain.color("accent"), Some(Rgba::rgb(0x0000ff)));
    assert_eq!(plain.option_values(), [("tone".to_owned(), OptionValue::Choice("cool".into()))]);
    let warm = r.resolve_with(&id, &[("tone".into(), "warm".into())]).unwrap();
    assert_eq!(warm.color("accent"), Some(Rgba::rgb(0xff8800)));
    assert_eq!(warm.color("text"), Some(Rgba::rgb(0xffffff)), "a token written as a color stays");
    let string = warm.syntax("string").unwrap();
    assert_eq!((string.foreground, string.italic), (Some(Rgba::rgb(0x88aa00)), true));
    assert_eq!(warm.options().len(), 1);
    // A value it doesn't take: the default.
    let wrong = r.resolve_with(&id, &[("tone".into(), "hot".into())]).unwrap();
    assert_eq!(wrong.color("accent"), Some(Rgba::rgb(0x0000ff)));
    // The child's option of the same id replaces it.
    let mut child = theme("child", Appearance::Dark, Some("base"), &[]);
    child.options.push(ThemeOption {
        id: "tone".into(),
        label: Label::plain("Tone"),
        kind: OptionKind::Toggle,
        default: OptionValue::Bool(true),
    });
    r.register(child);
    let child = ThemeId("child".into());
    let resolved = r.resolve(&child).unwrap();
    assert_eq!(resolved.options().len(), 1);
    assert_eq!(resolved.options()[0].kind, OptionKind::Toggle);
    let (_, declared) = r.declared(&id, &[("tone".into(), "warm".into())]).unwrap();
    assert_eq!(declared[0].default, OptionValue::Choice("warm".into()));
}

#[test]
fn the_backdrop_comes_through_extends() {
    use crate::backdrop::{BackdropKind, BackdropSpec};
    let mut r = ThemeRegistry::default();
    r.register(Theme {
        backdrop: BackdropSpec {
            style: Some(BackdropKind::Glass),
            blur: Some(3.0),
            ..BackdropSpec::default()
        },
        ..theme("base", Appearance::Dark, None, &[])
    });
    r.register(Theme {
        backdrop: BackdropSpec { opacity: Some(0.3), ..BackdropSpec::default() },
        ..theme("child", Appearance::Dark, Some("base"), &[])
    });
    let child = r.resolve(&ThemeId("child".into())).unwrap().backdrop();
    assert_eq!(child.style, Some(BackdropKind::Glass));
    assert_eq!(child.blur, Some(3.0));
    assert_eq!(child.opacity, Some(0.3));
    let plain = r.resolve(&ThemeId("base".into())).unwrap().backdrop();
    assert_eq!(plain.opacity, None);
}

#[test]
fn a_doc_comment_s_parts_come_from_the_code_colors_unless_named() {
    let style = |c: u32, italic: bool| TextStyle {
        foreground: Some(Rgba::rgb(c)),
        background: None,
        bold: false,
        italic,
    };
    let mut base = theme("t", Appearance::Dark, None, &[]);
    base.syntax.insert("comment".into(), style(0x777777, true));
    base.syntax.insert("type".into(), style(0x55aaff, false));
    base.syntax.insert("variable.parameter".into(), style(0xffaa55, false));
    let mut r = ThemeRegistry::default();
    r.register(base.clone());
    let t = r.resolve(&ThemeId("t".into())).unwrap();
    let tag = t.syntax("comment.doc.tag.php").unwrap();
    assert_eq!((tag.foreground, tag.bold, tag.italic), (Some(Rgba::rgb(0x777777)), true, true));
    let ty = t.syntax("comment.doc.type.php").unwrap();
    assert_eq!((ty.foreground, ty.italic), (Some(Rgba::rgb(0x55aaff)), true), "as types are");
    assert_eq!(t.syntax("comment.doc.variable.php").unwrap().foreground, Some(Rgba::rgb(0xffaa55)));
    // A theme naming one: its own.
    base.syntax.insert("comment.doc.tag".into(), style(0x00ff00, false));
    r.register(base);
    let t = r.resolve(&ThemeId("t".into())).unwrap();
    assert_eq!(t.syntax("comment.doc.tag.php").unwrap().foreground, Some(Rgba::rgb(0x00ff00)));
}
