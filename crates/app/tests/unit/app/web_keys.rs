use super::*;

fn press(key: Key, text: Option<&str>, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        repeat: false,
        modifiers,
        text: text.map(str::to_owned),
        numpad: false,
        physical: None,
    }
}

/// A letter: its down with Windows' and macOS' codes and its character, then the
/// character typed; let go, only its up.
#[test]
fn a_letter_goes_down_types_and_comes_up() {
    let mut event = press(Key::Character("a".into()), Some("a"), Modifiers::default());
    let sent: Vec<_> = key_events(&event).collect();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        (sent[0].action, sent[0].windows_code, sent[0].native_code),
        (KeyAction::Down, 0x41, 0x00)
    );
    assert_eq!((sent[1].action, sent[1].character), (KeyAction::Char, u32::from('a')));
    event.pressed = false;
    let sent: Vec<_> = key_events(&event).collect();
    assert_eq!((sent.len(), sent[0].action), (1, KeyAction::Up));
}

/// A shortcut (⌘ held) types nothing; its key still goes, with its modifiers.
#[test]
fn a_shortcut_types_nothing() {
    let meta = Modifiers { meta: true, ..Modifiers::default() };
    let sent: Vec<_> = key_events(&press(Key::Character("z".into()), Some("z"), meta)).collect();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].windows_code, sent[0].modifiers.0), (0x5A, PageModifiers::META));
}

/// A function key without text: its Windows code everywhere; on macOS AppKit's key code
/// and character for it too, so Chromium there knows it.
#[test]
fn an_arrow_carries_its_codes() {
    let sent: Vec<_> =
        key_events(&press(Key::Named(NamedKey::ArrowLeft), None, Modifiers::default())).collect();
    assert_eq!(sent.len(), 1);
    let expected = if cfg!(target_os = "macos") { (0x25, 0x7B, 0xF702) } else { (0x25, 0, 0) };
    assert_eq!((sent[0].windows_code, sent[0].native_code, sent[0].character), expected);
    let f12 = Codes { windows: 0x7B, mac: 0x6F, mac_character: 0xF70F };
    assert_eq!(codes(&Key::Named(NamedKey::F(12)), false), f12);
}

/// An input method's text: one character each, an emoji whole (the host splits it).
#[test]
fn typed_text_goes_as_characters() {
    let sent: Vec<_> = text_events("ă😀", PageModifiers(0)).collect();
    assert_eq!(sent.iter().map(|k| k.character).collect::<Vec<_>>(), [0x103, 0x1F600]);
}

/// Function keys past the last a system has a code for keep the last one's.
#[test]
fn function_keys_stop_at_the_systems_last() {
    let f24 = codes(&Key::Named(NamedKey::F(24)), false);
    assert_eq!((f24.windows, f24.mac_character), (0x87, 0xF71B));
    let f40 = codes(&Key::Named(NamedKey::F(40)), false);
    assert_eq!((f40.windows, f40.mac_character), (0x87, 0xF726), "F24 on Windows, F35 on macOS");
}

/// A shifted character of the US layout's places goes with the code of the key it's on
/// (`!` is `1`'s, `?` is `/`'s); the keypad's `*` keeps its own.
#[test]
fn a_shifted_character_has_its_keys_code() {
    assert_eq!(
        codes(&Key::Character("!".into()), false),
        codes(&Key::Character("1".into()), false)
    );
    assert_eq!(codes(&Key::Character("?".into()), false).windows, 0xBF);
    assert_eq!(codes(&Key::Character("\"".into()), false).windows, 0xDE);
    assert_eq!(codes(&Key::Character("*".into()), true).windows, 0x6A);
}

/// Another layout's character on a key (Cyrillic `ф` on the key of `a`): its codes are the
/// key's place, so a page's shortcut on A matches; what it typed is still its own.
#[test]
fn a_key_is_coded_by_its_place_on_any_layout() {
    let event = KeyEvent {
        physical: Some('a'),
        ..press(Key::Character("ф".into()), Some("ф"), Modifiers::default())
    };
    let sent: Vec<_> = key_events(&event).collect();
    let latin = press(Key::Character("a".into()), Some("a"), Modifiers::default());
    let a: Vec<_> = key_events(&latin).collect();
    assert_eq!(sent[0].windows_code, a[0].windows_code, "A's key code");
    assert_eq!(sent[0].native_code, a[0].native_code);
    assert_eq!(sent[1].character, u32::from('ф'), "what it typed stays its own");
}
