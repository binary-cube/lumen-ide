use super::*;

#[test]
fn parses_modifiers_and_keys() {
    let c = KeyChord::parse("CmdOrCtrl+Shift+S").unwrap();
    assert!(c.primary && c.shift && !c.alt && !c.control);
    assert_eq!(c.key, ChordKey::Char('s'));
    assert_eq!(KeyChord::parse("Alt+up").unwrap().key, ChordKey::Named("Up".into()));
    assert_eq!(KeyChord::parse("F3").unwrap().key, ChordKey::Named("F3".into()));
    assert_eq!(KeyChord::parse("Ctrl+Space").unwrap().key, ChordKey::Char(' '));
    assert!(KeyChord::parse("Shift+").is_err());
    assert!(KeyChord::parse("A+B").is_err());
}

#[test]
fn ctrl_is_primary_outside_macos() {
    let c = KeyChord::parse("Ctrl+Tab").unwrap();
    assert!(c.clone().for_platform(true).control);
    let p = c.for_platform(false);
    assert!(p.primary && !p.control);
}

#[test]
fn keymap_lookup() {
    const SPECS: &[CommandSpec] = &[CommandSpec {
        id: CommandId("file.save"),
        label: "Save",
        menu: Some(MenuAt::new("file.open", 10)),
        shortcuts: &["CmdOrCtrl+S"],
    }];
    let keymap = Keymap::from_specs(SPECS, true).unwrap();
    let chord = KeyChord::parse("CmdOrCtrl+S").unwrap();
    assert_eq!(keymap.lookup(&chord), Some(CommandId("file.save")));
    assert_eq!(keymap.lookup(&KeyChord::parse("CmdOrCtrl+Shift+S").unwrap()), None);
}

/// A key press is saved as a shortcut and read back to the same chord.
#[test]
fn a_chord_written_reads_back_the_same() {
    for spec in ["CmdOrCtrl+Shift+K", "Alt+Up", "F3", "Ctrl+Space", "CmdOrCtrl+Alt+["] {
        let chord = KeyChord::parse(spec).unwrap();
        assert_eq!(KeyChord::parse(&chord.spec()).unwrap(), chord, "{spec}");
    }
    assert_eq!(KeyChord::parse("cmdorctrl+shift+k").unwrap().spec(), "CmdOrCtrl+Shift+K");
}

/// The same keys left to two commands: both found (to choose from), the place's first.
#[test]
fn every_command_on_the_same_keys() {
    use crate::bindings::{Binding, Origin};
    let b = |id: &'static str, keys: &str, when| Binding {
        command: CommandId(id),
        shortcuts: vec![keys.into()],
        when,
        origin: Origin::User,
    };
    let keymap = Keymap::from_bindings(
        &[
            b("a", "CmdOrCtrl+J", When::Everywhere),
            b("b", "CmdOrCtrl+J", When::Everywhere),
            b("c", "CmdOrCtrl+J", When::View("canvas")),
        ],
        true,
    )
    .unwrap();
    let j = KeyChord::parse("CmdOrCtrl+J").unwrap();
    assert_eq!(keymap.lookup_all_in(&j, &[When::Editor]), [CommandId("a"), CommandId("b")]);
    assert_eq!(keymap.lookup_all_in(&j, &[When::View("canvas")]), [CommandId("c")]);
}
