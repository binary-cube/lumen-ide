//! The messages: what the app asks ([`Command`]) and what the host tells ([`Event`]), each
//! a small DTO that can grow (new fields at the end of a struct, new variants at the end of
//! an enum, the protocol's version raised).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A page, named by the app that opened it (unique within its connection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

/// What the app says first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The app's protocol ([`crate::PROTOCOL`]).
    pub protocol: u32,
    /// The app's process: where Windows' texture handles are duplicated to.
    pub pid: u32,
    /// macOS: the Mach service the app checked in, where the frames' surfaces are sent.
    pub mach_service: Option<String>,
    /// The GPU the app draws on (Windows: its adapter): a texture shared by the engine opens
    /// only on the GPU it was made on. `None`: any (one GPU, or the system shares across).
    pub adapter: Option<Adapter>,
}

/// A GPU, as the system names it between processes: Windows' adapter LUID (its two halves,
/// as DXGI gives them and Chromium's `--use-adapter-luid` takes them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Adapter {
    pub high: i32,
    pub low: u32,
}

impl Adapter {
    /// As Chromium's `--use-adapter-luid` takes it: `<high>,<low>`, both decimal (anything
    /// else is ignored by it, silently).
    pub fn switch_value(&self) -> String {
        format!("{},{}", self.high, self.low)
    }

    /// From [`Self::switch_value`]'s text (`None`: not one).
    pub fn parse(text: &str) -> Option<Self> {
        let (high, low) = text.split_once(',')?;
        Some(Self { high: high.trim().parse().ok()?, low: low.trim().parse().ok()? })
    }
}

/// The host's answer to [`Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    /// The host's protocol: another than the app's, the app lets the connection go.
    pub protocol: u32,
    /// The host's process (its texture handles come from there).
    pub pid: u32,
    /// The engine's version (`<Chromium>-<ours>`), for the log and the settings.
    pub engine: String,
    /// The GPU it draws on, when it was told one (an app on another can't open its frames).
    pub adapter: Option<Adapter>,
}

/// A page's size: physical pixels, and the screen's scale (CSS pixels are these over it).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

/// What a page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// HTML given (a response's): shown as it is, no script runs, nothing it links to is
    /// followed, no network.
    Html(String),
    /// A trusted app on this computer: `root`'s files served through the app scheme
    /// (nothing outside the folder, no network), its scripts running, messages both ways;
    /// `entry` the page opened first, relative to `root`, with its query.
    App { root: PathBuf, entry: String },
}

/// A page to open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub page: PageId,
    pub source: Source,
    pub size: Size,
    /// Opened hidden (a tab not in front): nothing drawn until shown.
    pub hidden: bool,
    /// Its background until it draws (RGBA), so it never flashes white over a dark theme.
    pub background: [u8; 4],
    /// Its frames as pixels in this file (the app's, in the channel's folder) rather than as
    /// GPU textures: where the app can't open the system's shared textures (another GPU, a
    /// graphics API without them, Linux until its dmabufs). `None`: GPU textures.
    pub pixels: Option<std::path::PathBuf>,
}

/// The keys held with an input (bits).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const SHIFT: u8 = 1;
    pub const CONTROL: u8 = 1 << 1;
    pub const ALT: u8 = 1 << 2;
    /// ⌘ on macOS, the Windows key elsewhere.
    pub const META: u8 = 1 << 3;
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseAction {
    Move,
    Down(Button),
    Up(Button),
    /// It left the page (the page's hover ends).
    Leave,
}

/// The mouse over a page, in its CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mouse {
    pub action: MouseAction,
    pub x: f32,
    pub y: f32,
    /// The press's count (1, 2 for a double click…).
    pub clicks: u8,
    pub modifiers: Modifiers,
}

/// The wheel or the trackpad over a page: where, and how far (CSS pixels).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wheel {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    pub modifiers: Modifiers,
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    Down,
    Up,
    /// The character it typed (after the layout: `character`).
    Char,
}

/// A key on a page: the engine's own codes (Windows' virtual key, the system's native one)
/// and the character it typed, as the system said them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub action: KeyAction,
    pub windows_code: i32,
    pub native_code: i32,
    pub character: u32,
    pub modifiers: Modifiers,
}

/// An editing action on a page, as its menu or its keys ask it (Chromium's own: on macOS
/// they come from the app's menu, never as keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// Files dragged from the system over a page (CSS pixels, in the page).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Drag {
    /// They came over it.
    Enter { files: Vec<std::path::PathBuf>, x: i32, y: i32 },
    /// They moved over it.
    Over { x: i32, y: i32 },
    /// They left it (or the drag was given up).
    Leave,
    /// They were dropped on it.
    Drop { x: i32, y: i32 },
}

/// Text from the system's input method, for the page with the keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ime {
    /// Being composed (shown underlined where the caret was), the part chosen in it as
    /// UTF-16 offsets (`None`: the caret at its end).
    Compose { text: String, selection: Option<(u32, u32)> },
    /// Typed: in the composition's place, or at the caret.
    Commit { text: String },
    /// The composition given up.
    Cancel,
}

/// What the app asks of the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Hello(Hello),
    Open(Open),
    Resize {
        page: PageId,
        size: Size,
    },
    /// Hidden (a tab behind another, a minimized window): the engine stops drawing it and
    /// slows its timers, its content kept; shown again, it's there at once.
    Hidden {
        page: PageId,
        hidden: bool,
    },
    Close {
        page: PageId,
    },
    /// Another HTML in an HTML page's place.
    LoadHtml {
        page: PageId,
        html: String,
    },
    /// A message for an app page (`window.__hostReceive(message)`).
    Post {
        page: PageId,
        message: String,
    },
    /// Draw a frame now if anything changed (the app asks only for pages on screen).
    BeginFrame {
        page: PageId,
    },
    /// The app copied frame `frame`: the engine may use its buffer again.
    FrameDone {
        page: PageId,
        frame: u64,
    },
    Mouse {
        page: PageId,
        mouse: Mouse,
    },
    Wheel {
        page: PageId,
        wheel: Wheel,
    },
    Key {
        page: PageId,
        key: Key,
    },
    /// The keyboard given to the page, or taken back.
    Focus {
        page: PageId,
        focused: bool,
    },
    /// An editing action on what the page has selected (copy, paste…).
    Edit {
        page: PageId,
        action: EditAction,
    },
    /// Text from the input method.
    Ime {
        page: PageId,
        ime: Ime,
    },
    /// Files dragged over it.
    Drag {
        page: PageId,
        drag: Drag,
    },
    /// Its whole view drawn again (it was shown again after its picture was let go).
    Repaint {
        page: PageId,
    },
    /// Its accessibility tree wanted (a screen reader runs) or not any more.
    Access {
        page: PageId,
        on: bool,
    },
}

/// A plane of a Linux frame's buffer (its file descriptor sent beside the message, in
/// order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plane {
    pub offset: u32,
    pub stride: u32,
}

/// Where a frame's GPU texture is: a handle the system passes between processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Texture {
    /// Windows: a D3D11 shared texture's NT handle, already duplicated into the app's
    /// process (the app closes it once copied).
    Windows { handle: u64 },
    /// macOS: an `IOSurface`, its Mach port sent on the app's Mach service with this
    /// frame's number.
    MachPort,
    /// Linux: a dmabuf, its planes' descriptors sent beside the message.
    Dmabuf { format: u32, modifier: u64, planes: Vec<Plane> },
    /// Its pixels written at the start of the page's file ([`Open::pixels`]): BGRA, rows
    /// `stride` bytes apart, premultiplied.
    Pixels { stride: u32 },
}

/// A new picture of a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub page: PageId,
    /// Its number (the app answers [`Command::FrameDone`] with it).
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub texture: Texture,
    /// Of its popup (an open `<select>`'s list: drawn apart, over the page where
    /// [`Event::Popup`] says), not of the page.
    pub popup: bool,
    /// What changed since the frame before it of the same kind (its pixels); empty: all of
    /// it (the first, one after a frame not handed over, or more than [`MAX_CHANGED`]).
    pub changed: Vec<Changed>,
}

/// The most changed rectangles a frame tells (more: it's told whole).
pub const MAX_CHANGED: usize = 16;

/// A rectangle of a frame that changed (its pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changed {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Changed {
    /// The rectangles `told` (as a frame's message says them: hostile until checked) as a
    /// frame of `width` × `height` can use them: each cut to the frame, one left with
    /// nothing in it dropped; none (the whole frame) when more than [`MAX_CHANGED`] were told.
    /// Empty means the whole frame, so nothing is ever left out by it.
    pub fn within(told: &[Changed], (width, height): (u32, u32)) -> Vec<Changed> {
        if told.len() > MAX_CHANGED {
            return Vec::new();
        }
        told.iter()
            .filter_map(|rect| {
                let cut = Changed {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width.min(width.saturating_sub(rect.x)),
                    height: rect.height.min(height.saturating_sub(rect.y)),
                };
                (cut.width > 0 && cut.height > 0).then_some(cut)
            })
            .collect()
    }
}

/// What a page's menu is asked for ([`Event::ContextMenu`]): where (CSS pixels, in the page),
/// whether text is selected there, whether it's in a field one types in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageMenu {
    pub x: i32,
    pub y: i32,
    pub selection: bool,
    pub editable: bool,
}

/// Where a page's popup is (CSS pixels, in the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopupRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A page's popup's pixels' file, beside its own ([`Open::pixels`]): `frames-1.bgra` →
/// `frames-1.popup.bgra`.
pub fn popup_pixels(path: &std::path::Path) -> std::path::PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("frames.bgra");
    let stem = name.strip_suffix(".bgra").unwrap_or(name);
    path.with_file_name(format!("{stem}.popup.bgra"))
}

/// The pointer's look a page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cursor {
    Arrow,
    Pointer,
    Text,
    Crosshair,
    Move,
    Grab,
    Grabbing,
    ResizeHorizontal,
    ResizeVertical,
    ResizeDiagonal,
    ResizeAntiDiagonal,
    NotAllowed,
    Wait,
    Progress,
    Help,
    None,
}

/// What the host tells the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Welcome(Welcome),
    Frame(Frame),
    Cursor {
        page: PageId,
        cursor: Cursor,
    },
    /// An app page's message (`window.ipc.postMessage(text)`).
    Message {
        page: PageId,
        text: String,
    },
    /// A link a page wanted opened: never followed by the engine, the app decides.
    OpenLink {
        page: PageId,
        url: String,
    },
    /// Its first content is there.
    Loaded {
        page: PageId,
    },
    /// Its popup shown where it is, or gone (`None`).
    Popup {
        page: PageId,
        rect: Option<PopupRect>,
    },
    /// A right click on it that asked for the menu (one it doesn't draw itself): the app
    /// shows its own, its rows the page's editing actions.
    ContextMenu {
        page: PageId,
        menu: PageMenu,
    },
    /// Where its text being composed is (CSS pixels, in the page): the input method's
    /// window goes beside it.
    Caret {
        page: PageId,
        rect: PopupRect,
    },
    /// Its engine process ended (a crash): the app shows it, the page can be opened again.
    Crashed {
        page: PageId,
    },
    /// Its accessibility tree (asked with `Command::Access`), whole, each time it changed.
    Access {
        page: PageId,
        nodes: Vec<AccessNode>,
    },
}

/// A node of a page's accessibility tree, as a screen reader needs it (parents before their
/// children).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessNode {
    /// Chromium's id for it: the same while it stays.
    pub id: i64,
    /// The node it's under (`None`: the page itself).
    pub parent: Option<i64>,
    /// Chromium's role (`button`, `link`, `heading`…).
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    /// Where it is (physical pixels in the page: x, y, width, height).
    pub rect: [f32; 4],
    pub focused: bool,
    pub disabled: bool,
    pub selected: bool,
    pub checked: Option<bool>,
    pub expanded: Option<bool>,
    /// A heading's level.
    pub level: Option<u8>,
}

#[cfg(test)]
#[path = "../tests/unit/messages.rs"]
mod tests;
