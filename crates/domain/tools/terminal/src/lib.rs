//! The Terminal context: shell sessions, the displayed character grid and the encoding
//! of keys into terminal sequences.
//!
//! The domain knows nothing about the PTY or the VT emulator; those live in infrastructure
//! (`pty`), behind the [`TerminalSession`] port.

pub mod history;
pub mod prompt;
pub mod scroll;
pub mod shells;
pub mod text;

use std::collections::HashMap;
use std::path::PathBuf;

/// The terminal size: cells and the size of one cell (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

/// A cell's color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    /// The default color (foreground or background, depending on context), from the theme.
    #[default]
    Default,
    /// One of the 256 indexed colors (0–15 = ANSI, from the theme).
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A cell's style (bits).
pub mod flags {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const INVERSE: u8 = 1 << 3;
    pub const DIM: u8 = 1 << 4;
    pub const STRIKE: u8 = 1 << 5;
    pub const HIDDEN: u8 = 1 << 6;
    /// Wide character (CJK, emoji): also occupies the next cell.
    pub const WIDE: u8 = 1 << 7;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermCell {
    pub ch: char,
    pub fg: TermColor,
    pub bg: TermColor,
    pub flags: u8,
}

impl Default for TermCell {
    fn default() -> Self {
        Self { ch: ' ', fg: TermColor::Default, bg: TermColor::Default, flags: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
    Hidden,
}

/// How the program wants keys and pastes sent (what a key press encodes with).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputModes {
    /// The arrows as `ESC O A` (DECCKM), not `ESC [ A`.
    pub app_cursor: bool,
    /// Pasted text wrapped in `ESC [200~` … `ESC [201~`.
    pub bracketed_paste: bool,
}

impl InputModes {
    /// The modes `grid` was copied with.
    pub fn of(grid: &GridSnapshot) -> Self {
        Self { app_cursor: grid.app_cursor, bracketed_paste: grid.bracketed_paste }
    }
}

/// What is visible in the terminal: `rows` × `cols` cells, row by row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GridSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<TermCell>,
    /// (row, column) in the visible area; `None` if outside it (scrolled into history).
    pub cursor: Option<(u16, u16)>,
    pub cursor_shape: CursorShape,
    /// How many lines it is scrolled back into history (0 = at the bottom).
    pub display_offset: usize,
    /// The program requested arrow keys in "application mode" (vim, less).
    pub app_cursor: bool,
    /// Pasted text must be wrapped in `ESC[200~ … ESC[201~`.
    pub bracketed_paste: bool,
    /// Colors redefined by programs (OSC 4 / 10 / 11), by index.
    pub palette_overrides: HashMap<u16, (u8, u8, u8)>,
    /// The lines kept above the screen (the scrollback; none on the alternate screen).
    pub history: usize,
    /// A full-screen program's own screen (vim, htop, less): no scrollback there.
    pub alt_screen: bool,
    /// The program asked for the mouse (its wheel is reported to it, not scrolled here).
    pub mouse: MouseReport,
    /// On the alternate screen, the wheel sent as ↑ / ↓ (the program scrolls itself).
    pub alternate_scroll: bool,
    /// Asked by the view before a snapshot: the history line just above the screen too
    /// ([`Self::above`]), drawn while it's scrolled by part of a line (see [`scroll`]).
    pub want_above: bool,
    /// That line's cells (`cols` of them; empty: not asked, or none above).
    pub above: Vec<TermCell>,
}

/// How a program asked for the mouse to be reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MouseReport {
    #[default]
    Off,
    /// The classic encoding (`ESC [ M` and three bytes).
    Normal,
    /// SGR's (`ESC [ < b ; x ; y M`), for any size.
    Sgr,
}

impl GridSnapshot {
    pub fn row(&self, row: u16) -> &[TermCell] {
        let start = row as usize * self.cols as usize;
        self.cells.get(start..start + self.cols as usize).unwrap_or(&[])
    }

    /// A row's text, without trailing spaces.
    pub fn row_text(&self, row: u16) -> String {
        let text: String = self.row(row).iter().map(|c| c.ch).collect();
        text.trim_end().to_owned()
    }
}

/// How a tab's shell is started.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellProfile {
    pub name: String,
    /// `None` = the user's default shell.
    pub program: Option<String>,
    pub args: Vec<String>,
    /// The start directory (the project root).
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

/// What the user chose for a session: the lines of history kept, the cursor's shape
/// unless the program asks for its own, and whether programs may erase the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermOptions {
    pub scrollback: usize,
    pub cursor: CursorShape,
    pub history_clear: history::HistoryClear,
}

impl Default for TermOptions {
    fn default() -> Self {
        Self {
            scrollback: 5_000,
            cursor: CursorShape::Block,
            history_clear: history::HistoryClear::default(),
        }
    }
}

/// A color as a terminal answers it: red, green, blue.
pub type Rgb8 = (u8, u8, u8);

/// The colors a terminal draws with, as programs may ask them (OSC 10 / 11 / 12 / 4: a
/// program choosing its own colors to suit a dark or a light background).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermPalette {
    pub foreground: Rgb8,
    pub background: Rgb8,
    pub cursor: Rgb8,
    /// The 16 ANSI colors (black … white, then the bright ones).
    pub ansi: [Rgb8; 16],
}

/// xterm's numbers for the default colors (after the 256 indexed ones).
pub const FOREGROUND_INDEX: usize = 256;
pub const BACKGROUND_INDEX: usize = 257;
pub const CURSOR_INDEX: usize = 258;

impl TermPalette {
    /// Color `index` as xterm numbers them: 0–15 the ANSI ones, 16–255 the fixed cube and
    /// grays, then the text, the background and the cursor; `None` past those.
    pub fn color(&self, index: usize) -> Option<Rgb8> {
        match index {
            0..=15 => Some(self.ansi[index]),
            16..=255 => Some(indexed_rgb(index as u8)),
            FOREGROUND_INDEX => Some(self.foreground),
            BACKGROUND_INDEX => Some(self.background),
            CURSOR_INDEX => Some(self.cursor),
            _ => None,
        }
    }
}

/// The fixed colors 16–255: the 6×6×6 cube, then 24 grays (below 16: black, the palette's
/// own).
pub fn indexed_rgb(i: u8) -> Rgb8 {
    match i {
        0..=15 => (0, 0, 0),
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}

/// The port to a terminal session (PTY + emulator), implemented in infrastructure.
pub trait TerminalSession: Send {
    /// Sends bytes to the shell (keys, pasted text).
    fn write(&self, bytes: &[u8]);
    fn resize(&mut self, size: TermSize);
    /// Copies what is visible now into `out` (reuses allocations).
    fn snapshot(&self, out: &mut GridSnapshot);
    /// [`Self::snapshot`] only if the emulator is free now (its reader isn't parsing the
    /// program's output): `false`, nothing copied — the caller keeps what it has and asks
    /// again at its next frame, never waiting for the program (the UI thread).
    fn try_snapshot(&self, out: &mut GridSnapshot) -> bool {
        self.snapshot(out);
        true
    }
    /// The input modes now, only if the emulator is free (as [`Self::try_snapshot`]): `None`
    /// while its reader parses — a key press then uses the last snapshot's, never waiting
    /// for the program on the UI thread, nor copying the screen to read two flags.
    fn try_modes(&self) -> Option<InputModes> {
        let mut grid = GridSnapshot::default();
        self.try_snapshot(&mut grid).then(|| InputModes::of(&grid))
    }
    /// Scrolls through history (positive = up).
    fn scroll(&mut self, lines: i32);
    /// [`Self::scroll`] only if the emulator is free now (as [`Self::try_snapshot`]):
    /// `false`, nothing scrolled — the caller scrolls at its next frame.
    fn try_scroll(&mut self, lines: i32) -> bool {
        self.scroll(lines);
        true
    }
    fn scroll_to_bottom(&mut self);
    /// Empties the history and the screen above the cursor's line (Clear Terminal: always,
    /// whatever programs may do).
    fn clear_history(&mut self) {}
    /// The title set by the shell (OSC 0 / 2), if any.
    fn title(&self) -> Option<String>;
    /// The exit code, after the process has ended.
    fn exit_status(&self) -> Option<i32>;
    /// `true` if new content appeared since the last call (and resets the flag).
    fn take_dirty(&self) -> bool;
    /// The user's options, applied to the running session.
    fn set_options(&mut self, options: TermOptions) {
        let _ = options;
    }
    /// The colors it draws with now, answered to programs that ask (sent again when the
    /// theme or the choice changes).
    fn set_palette(&self, palette: TermPalette) {
        let _ = palette;
    }
    /// The shell's process id (its CPU and memory count for the terminal), when known.
    fn pid(&self) -> Option<u32> {
        None
    }
    /// What its screen and history hold in memory (bytes, about).
    fn memory(&self) -> u64 {
        0
    }
    /// What it's doing, to ask from elsewhere while it lives (`None`: never told).
    fn watch(&self) -> Option<std::sync::Arc<dyn SessionWatch>> {
        None
    }
}

/// What a session is doing, asked from anywhere (each a cheap question to the system) —
/// e.g. before closing, whether a program the user started still runs in it. Once the
/// session is gone: [`history::Foreground::Unknown`], nothing.
pub trait SessionWatch: Send + Sync {
    /// Who has the terminal: the shell (at its prompt) or a program started from it.
    fn foreground(&self) -> history::Foreground;
    /// The program in the foreground's name, when the system tells.
    fn program(&self) -> Option<String>;
    /// The title set by the shell (OSC 0 / 2), if any.
    fn title(&self) -> Option<String>;
}

/// A key sent to the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKey {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1–F12.
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// The byte sequence for a key (xterm). `app_cursor`: arrow keys in "application
/// mode" (`ESC O A` instead of `ESC [ A`), requested by vim, less etc.
pub fn encode_key(key: TermKey, mods: TermModifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // The xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let csi = |final_byte: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{code};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let alt_prefix = |mut bytes: Vec<u8>| {
        if mods.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    Some(match key {
        TermKey::Char(c) if mods.ctrl => {
            let byte = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '?' | '8' => 0x7f,
                _ => return Some(alt_prefix(c.to_string().into_bytes())),
            };
            alt_prefix(vec![byte])
        }
        TermKey::Char(c) => alt_prefix(c.to_string().into_bytes()),
        // Shift+Enter: a new line without sending the line (ESC CR, the meta-Enter that
        // readline-style editors and Claude Code take as one: what configured terminals,
        // iTerm2 and WezTerm among them, send when the program hasn't asked for the kitty keyboard
        // protocol — the emulator here never turns it on). Alt / Option+Enter: the same.
        TermKey::Enter if mods.shift && !mods.ctrl => b"\x1b\r".to_vec(),
        TermKey::Enter => alt_prefix(vec![b'\r']),
        TermKey::Backspace if mods.ctrl => alt_prefix(vec![0x08]),
        TermKey::Backspace => alt_prefix(vec![0x7f]),
        TermKey::Tab if mods.shift => b"\x1b[Z".to_vec(),
        TermKey::Tab => alt_prefix(vec![b'\t']),
        TermKey::Escape => alt_prefix(vec![0x1b]),
        TermKey::Up => csi('A'),
        TermKey::Down => csi('B'),
        TermKey::Right => csi('C'),
        TermKey::Left => csi('D'),
        TermKey::Home => csi('H'),
        TermKey::End => csi('F'),
        TermKey::Insert => tilde(2),
        TermKey::Delete => tilde(3),
        TermKey::PageUp => tilde(5),
        TermKey::PageDown => tilde(6),
        TermKey::F(n @ 1..=4) => {
            let c = (b'P' + n - 1) as char;
            if modifier > 1 {
                format!("\x1b[1;{modifier}{c}").into_bytes()
            } else {
                format!("\x1bO{c}").into_bytes()
            }
        }
        TermKey::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        TermKey::F(_) => return None,
    })
}

/// What the mouse wheel sends to the program, `lines` of it (positive: up) over the cell
/// `at` (row, column): the program's own wheel reports when it asked for the mouse, ↑ / ↓
/// on the alternate screen (a full-screen program scrolls itself); `None`: the history
/// scrolls here instead.
pub fn encode_wheel(lines: i32, at: (u16, u16), grid: &GridSnapshot) -> Option<Vec<u8>> {
    if lines == 0 {
        return None;
    }
    let (count, up) = (lines.unsigned_abs().min(200) as usize, lines > 0);
    let (row, col) = (u32::from(at.0) + 1, u32::from(at.1) + 1);
    let button = if up { 64u32 } else { 65 };
    let one: Vec<u8> = match grid.mouse {
        MouseReport::Sgr => format!("\x1b[<{button};{col};{row}M").into_bytes(),
        MouseReport::Normal => {
            let byte = |v: u32| (32 + v).min(255) as u8;
            vec![0x1b, b'[', b'M', byte(button), byte(col), byte(row)]
        }
        MouseReport::Off if grid.alt_screen && grid.alternate_scroll => {
            let arrow = if up { 'A' } else { 'B' };
            let lead = if grid.app_cursor { 'O' } else { '[' };
            format!("\x1b{lead}{arrow}").into_bytes()
        }
        MouseReport::Off => return None,
    };
    Some(one.repeat(count))
}

/// The pasted text, wrapped for "bracketed paste" if the program requested it.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Terminals send Enter as `\r`; the `\n`s from the clipboard become `\r`.
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // No ESC inside: an `ESC[201~` would close the wrapping early, and any other
        // sequence would reach the program as keys (pasted text must stay text).
        let text = text.replace('\x1b', "");
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
