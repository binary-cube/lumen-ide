//! The key combination of an event, for the keymap lookup (AltGr's characters kept as text).

use super::*;

/// The key combination of an event, for the keymap lookup.
pub(super) fn chord(event: &KeyEvent) -> Option<KeyChord> {
    if altgr_text(event) {
        return None;
    }
    let key = match &event.key {
        // The keypad's + and − (Expand All / Collapse All), apart from the main row's.
        Key::Character(c) if event.numpad && (c == "+" || c == "-") => {
            ChordKey::Named(if c == "+" { "Numpadadd" } else { "Numpadsubtract" }.to_owned())
        }
        Key::Character(c) => {
            let c = c.chars().next()?.to_lowercase().next()?;
            // With Shift, the US layout reports `{`/`}` for the `[`/`]` keys.
            ChordKey::Char(match c {
                '{' => '[',
                '}' => ']',
                '+' => '=',
                '_' => '-',
                c => c,
            })
        }
        Key::Named(named) => ChordKey::Named(
            match named {
                NamedKey::ArrowUp => "Up",
                NamedKey::ArrowDown => "Down",
                NamedKey::ArrowLeft => "Left",
                NamedKey::ArrowRight => "Right",
                NamedKey::PageUp => "Pageup",
                NamedKey::PageDown => "Pagedown",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
                NamedKey::Enter => "Enter",
                NamedKey::Escape => "Escape",
                NamedKey::Tab => "Tab",
                NamedKey::Backspace => "Backspace",
                NamedKey::Delete => "Delete",
                NamedKey::Insert => "Insert",
                NamedKey::Space => return Some(chord_with(event, ChordKey::Char(' '))),
                NamedKey::F(n) => return Some(chord_with(event, ChordKey::Named(format!("F{n}")))),
            }
            .to_owned(),
        ),
        Key::Unidentified => return None,
    };
    Some(chord_with(event, key))
}

/// A character typed with AltGr (Windows and Linux report it as Ctrl + Alt): `{`, `@`, `\\`,
/// `ș` on German, Polish, Romanian keyboards… It's text, never a shortcut. A real
/// Ctrl + Alt shortcut types nothing.
pub(super) fn altgr_text(event: &KeyEvent) -> bool {
    let m = event.modifiers;
    !MAC && m.ctrl
        && m.alt
        && !m.meta
        && event.text.as_deref().is_some_and(|t| !t.is_empty() && !t.chars().any(char::is_control))
}

fn chord_with(event: &KeyEvent, key: ChordKey) -> KeyChord {
    let m = event.modifiers;
    KeyChord { primary: m.primary(), shift: m.shift, alt: m.alt, control: MAC && m.ctrl, key }
}
