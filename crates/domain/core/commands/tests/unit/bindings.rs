use super::*;
use crate::Keymap;

const SAVE: CommandId = CommandId("file.save");
const LINE: CommandId = CommandId("navigate.line");
const RENAME: CommandId = CommandId("refactor.rename");

const SPECS: &[CommandSpec] = &[
    CommandSpec { id: SAVE, label: "Save", menu: None, shortcuts: &["CmdOrCtrl+S"] },
    CommandSpec { id: LINE, label: "Go to Line", menu: None, shortcuts: &["CmdOrCtrl+L"] },
    CommandSpec { id: RENAME, label: "Rename", menu: None, shortcuts: &["Shift+F6"] },
];

/// The app's preset and a module's, for another command.
const PRESETS: &[PresetShortcuts] = &[
    PresetShortcuts { preset: "vscode", command: LINE, shortcuts: &["Ctrl+G"] },
    PresetShortcuts { preset: "vscode", command: RENAME, shortcuts: &["F2"] },
];

fn user(pairs: &[(&str, &[&str])]) -> UserShortcuts {
    UserShortcuts(
        pairs
            .iter()
            .map(|(id, keys)| ((*id).to_owned(), keys.iter().map(|k| (*k).to_owned()).collect()))
            .collect(),
    )
}

fn of(bindings: &[Binding], id: CommandId) -> (Vec<String>, Origin) {
    let b = bindings.iter().find(|b| b.command == id).unwrap();
    (b.shortcuts.clone(), b.origin)
}

/// Own → preset → user: each layer replaces a command's list; what isn't in it stays.
#[test]
fn the_layers_resolve_in_order() {
    let none = UserShortcuts::default();
    let layers = Layers { preset: "jetbrains", presets: PRESETS, user: &none, mac: true };
    let all = resolve(SPECS, &layers, |_| When::Everywhere);
    assert_eq!(of(&all, LINE), (vec!["CmdOrCtrl+L".to_owned()], Origin::Default));

    let layers = Layers { preset: "vscode", ..layers };
    let all = resolve(SPECS, &layers, |_| When::Everywhere);
    assert_eq!(of(&all, LINE), (vec!["Ctrl+G".to_owned()], Origin::Preset));
    assert_eq!(of(&all, RENAME).0, ["F2"], "a module's preset");
    assert_eq!(of(&all, SAVE).1, Origin::Default);

    let mine = user(&[("navigate.line", &["CmdOrCtrl+J", "CmdOrCtrl+L"]), ("file.save", &[])]);
    let all = resolve(SPECS, &Layers { user: &mine, ..layers }, |_| When::Everywhere);
    assert_eq!(of(&all, LINE), (vec!["CmdOrCtrl+J".into(), "CmdOrCtrl+L".into()], Origin::User));
    assert_eq!(of(&all, SAVE), (Vec::new(), Origin::User), "removed by the user");
    assert_eq!(of(&all, RENAME).0, ["F2"]);
}

/// The same keys in places active at once conflict; in separate places they don't; on
/// Windows / Linux `Ctrl` is `CmdOrCtrl`.
#[test]
fn conflicts_are_the_same_keys_in_the_same_place() {
    let mine = user(&[("refactor.rename", &["CmdOrCtrl+S"])]);
    let layers = Layers { preset: "default", presets: PRESETS, user: &mine, mac: true };
    let everywhere = resolve(SPECS, &layers, |_| When::Everywhere);
    let found = conflicts(&everywhere, true);
    assert_eq!(found, [Conflict { shortcut: "CmdOrCtrl+S".into(), commands: vec![SAVE, RENAME] }]);

    let apart = resolve(SPECS, &layers, |id| match id {
        RENAME => When::Editor,
        SAVE => When::View("terminal"),
        _ => When::Everywhere,
    });
    assert!(conflicts(&apart, true).is_empty());

    let mine = user(&[("refactor.rename", &["Ctrl+L"])]);
    let layers = Layers { user: &mine, ..layers };
    let all = resolve(SPECS, &layers, |_| When::Everywhere);
    assert!(conflicts(&all, true).is_empty(), "Ctrl isn't ⌘ on macOS");
    assert_eq!(conflicts(&all, false)[0].commands, [LINE, RENAME]);
}

/// A binding for the place the keys are in wins over one that works everywhere.
#[test]
fn the_place_s_binding_wins() {
    let mine = user(&[("refactor.rename", &["CmdOrCtrl+S"])]);
    let layers = Layers { preset: "default", presets: PRESETS, user: &mine, mac: true };
    let all =
        resolve(SPECS, &layers, |id| if id == RENAME { When::Editor } else { When::Everywhere });
    let keymap = Keymap::from_bindings(&all, true).unwrap();
    let save = KeyChord::parse("CmdOrCtrl+S").unwrap();
    assert_eq!(keymap.lookup_in(&save, &[When::Editor]), Some(RENAME));
    assert_eq!(keymap.lookup_in(&save, &[When::View("terminal")]), Some(SAVE));
    assert_eq!(keymap.lookup_in(&save, &[]), Some(SAVE));
    assert!(keymap.lookup_in(&KeyChord::parse("F9").unwrap(), &[When::Editor]).is_none());
}

/// A language's editor is a part of the editor: its keys conflict with the editor's, never
/// with a view's or another language's; looked up first, the editor's after.
#[test]
fn a_language_s_editor_is_a_part_of_the_editor() {
    let sql = When::EditorLanguage("sql");
    assert!(sql.overlaps(When::Editor) && When::Editor.overlaps(sql));
    assert!(sql.overlaps(When::Everywhere) && sql.overlaps(sql));
    assert!(!sql.overlaps(When::EditorLanguage("php")));
    assert!(!sql.overlaps(When::View("database.view")));
    let layers =
        Layers { preset: "default", presets: PRESETS, user: &UserShortcuts::default(), mac: true };
    let all = resolve(SPECS, &layers, |id| match id {
        RENAME => When::EditorLanguage("sql"),
        _ => When::Everywhere,
    });
    let keymap = Keymap::from_bindings(&all, true).unwrap();
    let keys = all.iter().find(|b| b.command == RENAME).and_then(|b| b.shortcuts.first()).cloned();
    if let Some(keys) = keys {
        let chord = KeyChord::parse(&keys).unwrap();
        assert_eq!(keymap.lookup_in(&chord, &[sql, When::Editor]), Some(RENAME));
        assert_ne!(keymap.lookup_in(&chord, &[When::Editor]), Some(RENAME));
    }
}

/// A shortcut for one system only is there alone: `mac:…` on macOS, `pc:…` elsewhere.
#[test]
fn a_shortcut_of_one_system_is_left_out_on_the_other() {
    const SPECS: &[CommandSpec] = &[CommandSpec {
        id: CommandId("edit.next"),
        label: "Next",
        menu: None,
        shortcuts: &["mac:Ctrl+G", "Alt+J", "pc:CmdOrCtrl+H"],
    }];
    let none = UserShortcuts::default();
    let layers = |mac| Layers { preset: "default", presets: &[], user: &none, mac };
    assert_eq!(layers(true).shortcuts(&SPECS[0]).0, ["Ctrl+G", "Alt+J"]);
    assert_eq!(layers(false).shortcuts(&SPECS[0]).0, ["Alt+J", "CmdOrCtrl+H"]);
    assert!(crate::Keymap::from_specs(SPECS, false).is_ok());
    let g = KeyChord::parse("Ctrl+G").unwrap();
    assert_eq!(
        crate::Keymap::from_specs(SPECS, false).unwrap().lookup(&g.for_platform(false)),
        None
    );
}
