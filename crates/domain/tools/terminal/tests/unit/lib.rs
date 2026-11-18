use super::*;

const NONE: TermModifiers = TermModifiers { shift: false, ctrl: false, alt: false };

fn key(k: TermKey) -> Vec<u8> {
    encode_key(k, NONE, false).unwrap()
}

#[test]
fn plain_keys() {
    assert_eq!(key(TermKey::Char('ă')), "ă".as_bytes());
    assert_eq!(key(TermKey::Enter), b"\r");
    assert_eq!(key(TermKey::Backspace), b"\x7f");
    assert_eq!(key(TermKey::Up), b"\x1b[A");
    assert_eq!(key(TermKey::Delete), b"\x1b[3~");
    assert_eq!(key(TermKey::F(1)), b"\x1bOP");
    assert_eq!(key(TermKey::F(12)), b"\x1b[24~");
}

#[test]
fn modifiers_and_application_cursor() {
    let ctrl = TermModifiers { ctrl: true, ..NONE };
    let alt = TermModifiers { alt: true, ..NONE };
    assert_eq!(encode_key(TermKey::Char('c'), ctrl, false).unwrap(), [3]);
    assert_eq!(encode_key(TermKey::Char('b'), alt, false).unwrap(), b"\x1bb");
    assert_eq!(encode_key(TermKey::Left, alt, false).unwrap(), b"\x1b[1;3D");
    assert_eq!(encode_key(TermKey::Up, NONE, true).unwrap(), b"\x1bOA");
    let shift = TermModifiers { shift: true, ..NONE };
    assert_eq!(encode_key(TermKey::Tab, shift, false).unwrap(), b"\x1b[Z");
}

#[test]
fn paste_is_bracketed_when_requested() {
    assert_eq!(encode_paste("a\nb", false), b"a\rb");
    assert_eq!(encode_paste("ls\n", true), b"\x1b[200~ls\r\x1b[201~");
}

#[test]
fn snapshot_rows() {
    let mut grid = GridSnapshot { cols: 3, rows: 2, ..Default::default() };
    grid.cells = vec![TermCell::default(); 6];
    grid.cells[3].ch = 'o';
    grid.cells[4].ch = 'k';
    assert_eq!(grid.row_text(1), "ok");
    assert_eq!(grid.row_text(0), "");
}

#[test]
fn the_wheel_goes_to_the_program_that_asked_for_it_and_scrolls_the_history_otherwise() {
    let mut grid = GridSnapshot::default();
    // The normal screen, no mouse asked for: the history scrolls here.
    assert_eq!(encode_wheel(3, (0, 0), &grid), None);
    // A full-screen program: the wheel as arrows (application mode as asked).
    grid.alt_screen = true;
    grid.alternate_scroll = true;
    assert_eq!(encode_wheel(2, (0, 0), &grid).unwrap(), b"\x1b[A\x1b[A");
    grid.app_cursor = true;
    assert_eq!(encode_wheel(-1, (0, 0), &grid).unwrap(), b"\x1bOB");
    // Without alternate scroll: nothing (no history there either).
    grid.alternate_scroll = false;
    assert_eq!(encode_wheel(1, (0, 0), &grid), None);
    // The program asked for the mouse: its wheel reports at the cell, SGR or classic.
    grid.mouse = MouseReport::Sgr;
    assert_eq!(encode_wheel(1, (4, 9), &grid).unwrap(), b"\x1b[<64;10;5M");
    assert_eq!(encode_wheel(-2, (0, 0), &grid).unwrap(), b"\x1b[<65;1;1M\x1b[<65;1;1M");
    grid.mouse = MouseReport::Normal;
    assert_eq!(encode_wheel(1, (1, 2), &grid).unwrap(), [0x1b, b'[', b'M', 96, 35, 34]);
    assert_eq!(encode_wheel(0, (1, 2), &grid), None);
}

#[test]
fn shift_or_alt_enter_is_a_new_line_without_sending_the_line() {
    let shift = TermModifiers { shift: true, ..NONE };
    let alt = TermModifiers { alt: true, ..NONE };
    assert_eq!(encode_key(TermKey::Enter, shift, false).unwrap(), b"\x1b\r");
    assert_eq!(encode_key(TermKey::Enter, alt, false).unwrap(), b"\x1b\r");
    assert_eq!(encode_key(TermKey::Enter, NONE, true).unwrap(), b"\r", "plain Enter unchanged");
}

#[test]
fn a_bracketed_paste_carries_no_escape_sequences() {
    // A sequence hidden in the clipboard (closing the paste, or a key) stays text.
    let pasted = encode_paste("echo hi\x1b[201~\x1b[Arm -rf ~\r\n", true);
    assert_eq!(pasted, b"\x1b[200~echo hi[201~[Arm -rf ~\r\x1b[201~");
}

/// Colors are numbered as xterm does: the 16 ANSI, the cube, the grays, then the text, the
/// background and the cursor.
#[test]
fn a_palette_s_colors_are_numbered_as_xterm_s() {
    let mut ansi = [(0, 0, 0); 16];
    ansi[1] = (200, 0, 0);
    let p = TermPalette { foreground: (1, 1, 1), background: (2, 2, 2), cursor: (3, 3, 3), ansi };
    assert_eq!(p.color(1), Some((200, 0, 0)));
    assert_eq!(p.color(16), Some((0, 0, 0)));
    assert_eq!(p.color(21), Some((0, 0, 255)));
    assert_eq!(p.color(231), Some((255, 255, 255)));
    assert_eq!(p.color(232), Some((8, 8, 8)));
    assert_eq!(p.color(255), Some((238, 238, 238)));
    assert_eq!(p.color(FOREGROUND_INDEX), Some((1, 1, 1)));
    assert_eq!(p.color(BACKGROUND_INDEX), Some((2, 2, 2)));
    assert_eq!(p.color(CURSOR_INDEX), Some((3, 3, 3)));
    assert_eq!(p.color(259), None);
}
