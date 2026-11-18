use super::*;

const ALT: Modifiers = Modifiers { shift: false, ctrl: false, alt: true, meta: false };
const CTRL_ALT: Modifiers = Modifiers { shift: false, ctrl: true, alt: true, meta: false };
const NONE: Modifiers = Modifiers { shift: false, ctrl: false, alt: false, meta: false };

#[test]
fn a_lone_alt_released_fires_once() {
    let mut tap = AltTap::default();
    assert!(!tap.modifiers(ALT));
    assert!(tap.modifiers(NONE));
    assert!(!tap.modifiers(NONE), "nothing held: nothing more");
}

#[test]
fn a_key_a_click_or_another_modifier_in_between_does_not_fire() {
    let mut tap = AltTap::default();
    tap.modifiers(ALT);
    tap.cancel();
    assert!(!tap.modifiers(NONE), "Alt + a key (a shortcut)");
    // AltGr (Ctrl + Alt) typing a character.
    tap.modifiers(ALT);
    tap.modifiers(CTRL_ALT);
    assert!(!tap.modifiers(NONE));
    // Ctrl first, then Alt, then both released.
    tap.modifiers(Modifiers { ctrl: true, ..NONE });
    tap.modifiers(CTRL_ALT);
    assert!(!tap.modifiers(NONE));
}

#[test]
fn alt_held_across_a_window_switch_does_not_fire() {
    let mut tap = AltTap::default();
    // Alt + Tab into the window: the focus comes with Alt held (in either order).
    tap.modifiers(ALT);
    tap.focus_changed();
    assert!(!tap.modifiers(ALT));
    assert!(!tap.modifiers(NONE), "released: nothing");
    // Then a real lone Alt works again.
    tap.modifiers(ALT);
    assert!(tap.modifiers(NONE));
}
