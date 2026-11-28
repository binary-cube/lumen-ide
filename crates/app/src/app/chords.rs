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
        // A modifier alone is no shortcut.
        Key::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Meta) => {
            return None;
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
                NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Meta => {
                    return None;
                }
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

/// What a key does with the clipboard in a view taking the whole keyboard (the terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalClip {
    Copy,
    Paste,
}

/// The system the terminal's clipboard keys follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeysOf {
    Mac,
    Windows,
    Linux,
}

impl KeysOf {
    /// The system this build runs on.
    pub(super) const HERE: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Linux
    };
}

/// The terminal's clipboard keys, as the system's own terminals have them: on macOS none
/// (⌘C / ⌘V are the app's Copy and Paste); elsewhere Ctrl+Shift+C / V and Shift+Insert,
/// Ctrl+C when something is selected (else it interrupts the program), and on Windows
/// Ctrl+V too (elsewhere it stays the program's: a shell's literal next key).
/// `selected` is asked only for Ctrl+C (it reads the view's selection).
pub(super) fn terminal_clip(
    event: &KeyEvent,
    selected: impl FnOnce() -> bool,
    keys: KeysOf,
) -> Option<TerminalClip> {
    if keys == KeysOf::Mac {
        return None;
    }
    let m = event.modifiers;
    let letter = |l: char| matches!(&event.key, Key::Character(c) if c.chars().next().is_some_and(|c| c.eq_ignore_ascii_case(&l)) && c.chars().count() == 1);
    if m.ctrl && !m.alt && letter('c') && (m.shift || selected()) {
        return Some(TerminalClip::Copy);
    }
    let ctrl_v = m.ctrl && !m.alt && letter('v') && (m.shift || keys == KeysOf::Windows);
    let insert = m.shift && !m.ctrl && matches!(&event.key, Key::Named(NamedKey::Insert));
    (ctrl_v || insert).then_some(TerminalClip::Paste)
}
