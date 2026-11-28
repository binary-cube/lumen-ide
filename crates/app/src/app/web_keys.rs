//! The keys of the window as a page of the app's engine takes them: what the key is to
//! Windows (its virtual-key code, which pages see as `keyCode` everywhere) and to macOS
//! (its key code and the character AppKit gives it, which Chromium reads there), then the
//! text it typed as characters. Pure: what's sent is decided here, sent by the page.

use platform_api::{Key, KeyEvent, Modifiers, NamedKey};
use web_engine::protocol::{Key as PageKey, KeyAction, Modifiers as PageModifiers};

/// A key's codes: Windows' virtual key, macOS' key code, and the character AppKit gives a
/// key without text (a function key's private-use one; 0: none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Codes {
    pub windows: i32,
    pub mac: i32,
    pub mac_character: u32,
}

/// The modifiers as the protocol carries them.
pub(super) fn modifiers(m: Modifiers) -> PageModifiers {
    let mut bits = 0;
    for (held, bit) in [
        (m.shift, PageModifiers::SHIFT),
        (m.ctrl, PageModifiers::CONTROL),
        (m.alt, PageModifiers::ALT),
        (m.meta, PageModifiers::META),
    ] {
        if held {
            bits |= bit;
        }
    }
    PageModifiers(bits)
}

/// What `event` sends a page: the key down (or up), then, pressed, each character it typed
/// (not for a shortcut: ⌘ or Ctrl on macOS, Ctrl without Alt elsewhere — Ctrl + Alt is
/// AltGr there, which types). Nothing gathered: each made as it's sent.
pub(super) fn key_events(event: &KeyEvent) -> impl Iterator<Item = PageKey> + '_ {
    let codes = place_codes(event);
    let modifiers = modifiers(event.modifiers);
    let m = event.modifiers;
    let shortcut = match cfg!(target_os = "macos") {
        true => m.meta || m.ctrl,
        false => m.ctrl && !m.alt,
    };
    let typed = event.text.as_deref().filter(|_| !shortcut);
    // macOS' Chromium reads AppKit's key code and character (a function key's own);
    // elsewhere the Windows code says it all.
    let mac = cfg!(target_os = "macos");
    let native_code = if mac { codes.mac } else { 0 };
    let function_character = if mac { codes.mac_character } else { 0 };
    // The press's own character: what it typed, else (macOS) AppKit's for a function key.
    let character = typed.and_then(|t| t.chars().next()).map_or(function_character, u32::from);
    let action = if event.pressed { KeyAction::Down } else { KeyAction::Up };
    let windows_code = codes.windows;
    let key = PageKey { action, windows_code, native_code, character, modifiers };
    let text = typed.filter(|_| event.pressed).unwrap_or_default();
    std::iter::once(key).chain(text_events(text, modifiers))
}

/// Text typed (an input method's), one character event each.
pub(super) fn text_events(
    text: &str,
    modifiers: PageModifiers,
) -> impl Iterator<Item = PageKey> + '_ {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\r' | '\t')).map(move |c| PageKey {
        action: KeyAction::Char,
        windows_code: i32::try_from(u32::from(c)).unwrap_or(0),
        native_code: 0,
        character: u32::from(c),
        modifiers,
    })
}

/// `event`'s codes: a character key's by its place (what a page's shortcuts match, on any
/// layout: `ф` or `@` on the key of `a` is `a`), else by what it typed.
fn place_codes(event: &KeyEvent) -> Codes {
    match (&event.key, event.physical, event.numpad) {
        (Key::Character(_), Some(place), false) => {
            let (windows, mac) = character_codes(place, false);
            Codes { windows, mac, mac_character: 0 }
        }
        _ => codes(&event.key, event.numpad),
    }
}

/// The codes of `key` (`numpad`: a key of the numeric keypad).
pub(super) fn codes(key: &Key, numpad: bool) -> Codes {
    let (windows, mac, mac_character) = match key {
        Key::Named(named) => named_codes(*named),
        Key::Character(text) => {
            let Some(c) = text.chars().next() else { return Codes::default() };
            // The keypad's own `*` and `+` are keys of their own, not a shifted one's.
            let c = if numpad { c } else { unshifted(c) };
            let (windows, mac) = character_codes(c.to_ascii_lowercase(), numpad);
            (windows, mac, 0)
        }
        Key::Unidentified => (0, 0, 0),
    };
    Codes { windows, mac, mac_character }
}

/// A named key's Windows code, macOS code and AppKit character.
fn named_codes(key: NamedKey) -> (i32, i32, u32) {
    match key {
        NamedKey::ArrowUp => (0x26, 0x7E, 0xF700),
        NamedKey::ArrowDown => (0x28, 0x7D, 0xF701),
        NamedKey::ArrowLeft => (0x25, 0x7B, 0xF702),
        NamedKey::ArrowRight => (0x27, 0x7C, 0xF703),
        NamedKey::PageUp => (0x21, 0x74, 0xF72C),
        NamedKey::PageDown => (0x22, 0x79, 0xF72D),
        NamedKey::Home => (0x24, 0x73, 0xF729),
        NamedKey::End => (0x23, 0x77, 0xF72B),
        NamedKey::Insert => (0x2D, 0x72, 0xF727),
        NamedKey::Delete => (0x2E, 0x75, 0xF728),
        NamedKey::Enter => (0x0D, 0x24, 0x0D),
        NamedKey::Escape => (0x1B, 0x35, 0x1B),
        NamedKey::Tab => (0x09, 0x30, 0x09),
        NamedKey::Backspace => (0x08, 0x33, 0x7F),
        NamedKey::Space => (0x20, 0x31, 0x20),
        NamedKey::F(n) => function_codes(n),
        // Their left ones' (AppKit: Shift, Control, Option, Command; no character).
        NamedKey::Shift => (0x10, 0x38, 0),
        NamedKey::Control => (0x11, 0x3B, 0),
        NamedKey::Alt => (0x12, 0x3A, 0),
        NamedKey::Meta => (0x5B, 0x37, 0),
    }
}

/// F1…F12's macOS key codes (not in order on its keyboard).
const MAC_FUNCTION_KEYS: [i32; 12] =
    [0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F];

/// Windows' first function key's code (F1), the rest following it.
const WINDOWS_F1: i32 = 0x70;

/// AppKit's first function key's character (F1), the rest following it.
const MAC_F1_CHARACTER: u32 = 0xF704;

/// The last function key Windows has a code for: F24 (from F1, 0).
const WINDOWS_LAST_FUNCTION: u8 = 23;

/// The last function key AppKit has a character for: F35 (from F1, 0).
const MAC_LAST_FUNCTION: u8 = 34;

/// Function key `n` (F1 = 1): Windows' codes run from [`WINDOWS_F1`], AppKit's characters
/// from [`MAC_F1_CHARACTER`] (past their last key, the last one).
fn function_codes(n: u8) -> (i32, i32, u32) {
    let index = n.saturating_sub(1);
    let mac = MAC_FUNCTION_KEYS.get(usize::from(index)).copied().unwrap_or(0);
    let windows = WINDOWS_F1 + i32::from(index.min(WINDOWS_LAST_FUNCTION));
    (windows, mac, MAC_F1_CHARACTER + u32::from(index.min(MAC_LAST_FUNCTION)))
}

/// macOS' key codes of the letters a…z (the US layout's places, which its shortcuts name).
const MAC_LETTERS: [i32; 26] = [
    0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F, 0x23,
    0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
];

/// macOS' key codes of the digits 0…9 (the row above the letters).
const MAC_DIGITS: [i32; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];

/// macOS' key codes of the keypad's digits 0…9.
const MAC_KEYPAD_DIGITS: [i32; 10] = [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5B, 0x5C];

/// The key a shifted character of the US layout's places is on (`!` is on `1`), so a page
/// sees which key it was (its `keyCode`); any other character as it is. Used where the key's
/// place isn't known ([`KeyEvent::physical`] `None`: a key told by its character only, the
/// codes of a key named): a non-Latin layout's letters then have no code (their text is
/// typed all the same).
fn unshifted(c: char) -> char {
    match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        ':' => ';',
        '+' => '=',
        '<' => ',',
        '_' => '-',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        '{' => '[',
        '|' => '\\',
        '}' => ']',
        '"' => '\'',
        c => c,
    }
}

/// A character key's Windows and macOS codes (a lowercase letter, a digit, punctuation of
/// the US layout's places; anything else: none, its text still typed).
fn character_codes(c: char, numpad: bool) -> (i32, i32) {
    let at = |base: char| (u32::from(c) - u32::from(base)) as usize;
    match c {
        'a'..='z' => (0x41 + at('a') as i32, MAC_LETTERS[at('a')]),
        '0'..='9' if numpad => (0x60 + at('0') as i32, MAC_KEYPAD_DIGITS[at('0')]),
        '0'..='9' => (0x30 + at('0') as i32, MAC_DIGITS[at('0')]),
        '+' if numpad => (0x6B, 0x45),
        '-' if numpad => (0x6D, 0x4E),
        '*' if numpad => (0x6A, 0x43),
        '/' if numpad => (0x6F, 0x4B),
        '.' if numpad => (0x6E, 0x41),
        ';' => (0xBA, 0x29),
        '=' => (0xBB, 0x18),
        ',' => (0xBC, 0x2B),
        '-' => (0xBD, 0x1B),
        '.' => (0xBE, 0x2F),
        '/' => (0xBF, 0x2C),
        '`' => (0xC0, 0x32),
        '[' => (0xDB, 0x21),
        '\\' => (0xDC, 0x2A),
        ']' => (0xDD, 0x1E),
        '\'' => (0xDE, 0x27),
        _ => (0, 0),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/web_keys.rs"]
mod tests;
