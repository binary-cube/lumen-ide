//! SVG icons for the UI.
//!
//! Icons are rasterized (resvg) as **masks**: the shape gives the alpha, and the color
//! comes from the theme at draw time. They go through the same glyph atlas as text, so
//! the renderer needs nothing special. Their keys have the top bit set
//! ([`is_icon`]), so they don't mix with text glyphs.
//!
//! The bundled set is drawn in the Lucide style (2px strokes, round caps, 24×24).
//! Modules can register their own icons with [`Icons::register`].
//!
//! The same drawings come in a few styles ([`IconStyle`]: thinner, bolder, duotone,
//! sharp), chosen in the settings — ours, not user-supplied files. A style rewrites the
//! SVG attributes; each style's shapes are parsed only when used.
//!
//! Whole packs with other drawings ([`IconPack`]: Lucide, Tabler, Phosphor…) come from
//! modules: the chosen one draws what it has (asked when needed), ours the rest — an icon
//! is never missing. The set drawn is an [`IconLook`]: one of our styles, or a pack; it's
//! in the glyph keys, so two looks never share a rasterized glyph.

mod sketch;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use render_api::{GlyphFormat, GlyphKey, GlyphRasterizer, RasterizedGlyph};
use resvg::{tiny_skia, usvg};

const FLAG: u64 = 1 << 63;

/// A pack of drawings for the interface's icons (a module's, licensed for it): its drawing
/// of each of our icon names it has. Asked when an icon is drawn, so it reads its data
/// only once chosen; let go when another is chosen ([`IconPack::release`]).
pub trait IconPack: Send + Sync + std::fmt::Debug {
    /// Its id, saved in the settings (`lucide`, `tabler`…).
    fn id(&self) -> &'static str;
    /// Its name, as shown (`Lucide`).
    fn name(&self) -> &'static str;
    /// Its author and license (`Lucide Contributors · ISC`).
    fn credit(&self) -> &'static str;
    /// Its drawing of icon `name` (one of ours: `search`, `folder`…), if it has one: an
    /// SVG whose shapes give the alpha (any color, filled or stroked).
    fn svg(&self, name: &str) -> Option<std::borrow::Cow<'static, str>>;
    /// Not drawn any more: what it read can go.
    fn release(&self) {}
}

/// A set of colored file icons (a module's, licensed for it): the icon of a file (by its
/// name, its language) and of a folder (by its name, open or not), as keys of its drawings —
/// drawn in their own colors. Asked when drawn; let go when another set is chosen.
pub trait FileIconSet: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn credit(&self) -> &'static str;
    /// Its icon for file `name` whose language is `language` (`php`, `` for none).
    fn file(&self, name: &str, language: &str) -> Option<&'static str>;
    /// Its icon for folder `name`, open or not.
    fn folder(&self, name: &str, open: bool) -> Option<&'static str>;
    /// Its drawing `key`: an SVG in its own colors.
    fn svg(&self, key: &str) -> Option<std::borrow::Cow<'static, str>>;
    /// Not drawn any more: what it read can go.
    fn release(&self) {}
}

/// A colored icon of a file icon set, given an id when first asked (its drawing parsed on
/// first draw).
struct Colored {
    set: u8,
    key: &'static str,
    tree: Mutex<Option<Option<Arc<usvg::Tree>>>>,
}

/// How an icon is colored where it's drawn: by a language (the theme's color for it), as a
/// folder (the theme's), or in its own colors (a file icon set's: the color's alpha only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tint {
    Language(String),
    Folder,
    Own,
}

/// The ids of colored icons: past every icon of ours.
const COLORED: u16 = 0x8000;

/// The set drawn: one of our styles, or a pack (by its place among those registered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconLook {
    Style(IconStyle),
    Pack(u8),
}

impl Default for IconLook {
    fn default() -> Self {
        Self::Style(IconStyle::default())
    }
}

impl IconLook {
    /// In a glyph key: a style's index, or a pack's place past the styles.
    fn code(self) -> u8 {
        match self {
            Self::Style(style) => style.index(),
            Self::Pack(i) => PACKS_FROM + i,
        }
    }

    fn from_code(code: u8) -> Self {
        if code >= PACKS_FROM {
            Self::Pack(code - PACKS_FROM)
        } else {
            Self::Style(IconStyle::from_index(code))
        }
    }
}

/// The looks' codes past our styles' are packs'.
const PACKS_FROM: u8 = 16;

/// A style of the icon set: the same drawings, drawn differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum IconStyle {
    /// 2px strokes, round caps (the drawings as they are).
    #[default]
    Regular,
    /// 1.5px strokes: airier.
    Light,
    /// 2.5px strokes: firmer.
    Bold,
    /// A soft fill inside the closed shapes.
    Duotone,
    /// Square caps, mitered joins, nearly square corners.
    Sharp,
    /// Drawn as if by hand: lines traced twice, wavering, closed shapes hatched.
    Sketch,
}

impl IconStyle {
    pub const ALL: [IconStyle; 6] = [
        IconStyle::Regular,
        IconStyle::Light,
        IconStyle::Bold,
        IconStyle::Duotone,
        IconStyle::Sharp,
        IconStyle::Sketch,
    ];

    /// The id saved in the settings.
    pub fn id(self) -> &'static str {
        match self {
            IconStyle::Regular => "regular",
            IconStyle::Light => "light",
            IconStyle::Bold => "bold",
            IconStyle::Duotone => "duotone",
            IconStyle::Sharp => "sharp",
            IconStyle::Sketch => "sketch",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        // `lumen`: the regular style's id in settings saved before it was renamed.
        let id = if id == "lumen" { "regular" } else { id };
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    fn index(self) -> u8 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8
    }

    fn from_index(i: u8) -> Self {
        Self::ALL.get(usize::from(i)).copied().unwrap_or_default()
    }

    /// The SVG drawn in this style.
    pub fn restyle(self, svg: &str) -> String {
        match self {
            IconStyle::Regular => svg.to_owned(),
            IconStyle::Light => scale_attr(svg, "stroke-width", 0.75),
            IconStyle::Bold => scale_attr(svg, "stroke-width", 1.25),
            IconStyle::Duotone => fill_closed(svg),
            IconStyle::Sharp => {
                let svg = svg
                    .replace("stroke-linecap=\"round\"", "stroke-linecap=\"square\"")
                    .replace("stroke-linejoin=\"round\"", "stroke-linejoin=\"miter\"");
                scale_attr(&scale_attr(&svg, "rx", 0.3), "ry", 0.3)
            }
            IconStyle::Sketch => sketch::sketch(svg).unwrap_or_else(|| svg.to_owned()),
        }
    }
}

/// Every numeric `name="…"` attribute multiplied by `k`.
fn scale_attr(svg: &str, name: &str, k: f32) -> String {
    let pattern = format!(" {name}=\"");
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find(&pattern) {
        let start = at + pattern.len();
        out.push_str(&rest[..start]);
        let end = rest[start..].find('"').map_or(rest.len(), |e| start + e);
        match rest[start..end].parse::<f32>() {
            Ok(v) => out.push_str(&format!("{}", (v * k * 100.0).round() / 100.0)),
            Err(_) => out.push_str(&rest[start..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A soft fill inside the closed shapes (rectangles, circles, closed paths) without a fill
/// of their own; open strokes (chevrons, lines) stay as they are.
fn fill_closed(svg: &str) -> String {
    const FILL: &str = " fill=\"#000\" fill-opacity=\"0.22\"";
    let mut out = String::with_capacity(svg.len() + 64);
    let mut rest = svg;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let end = rest[at..].find('>').map_or(rest.len(), |e| at + e + 1);
        let element = &rest[at..end];
        let tag = element[1..].split([' ', '/', '>']).next().unwrap_or("");
        let closed = match tag {
            "rect" | "circle" | "ellipse" | "polygon" => true,
            "path" => element
                .split(" d=\"")
                .nth(1)
                .and_then(|d| d.split('"').next())
                .is_some_and(|d| d.trim_end().ends_with(['z', 'Z'])),
            _ => false,
        };
        if closed && !element.contains(" fill=") {
            out.push('<');
            out.push_str(tag);
            out.push_str(FILL);
            out.push_str(&element[1 + tag.len()..]);
        } else {
            out.push_str(element);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Bundled icons: (name, SVG source).
const BUILTIN: &[(&str, &str)] = &[
    ("alert", include_str!("../svg/alert.svg")),
    ("folder", include_str!("../svg/folder.svg")),
    ("folder-plus", include_str!("../svg/folder-plus.svg")),
    ("folder-x", include_str!("../svg/folder-x.svg")),
    ("pin", include_str!("../svg/pin.svg")),
    ("globe", include_str!("../svg/globe.svg")),
    ("palette", include_str!("../svg/palette.svg")),
    ("file-pen", include_str!("../svg/file-pen.svg")),
    ("view-editor", include_str!("../svg/view-editor.svg")),
    ("view-split", include_str!("../svg/view-split.svg")),
    ("view-preview", include_str!("../svg/view-preview.svg")),
    ("folder-open", include_str!("../svg/folder-open.svg")),
    ("file", include_str!("../svg/file.svg")),
    ("file-code", include_str!("../svg/file-code.svg")),
    ("file-json", include_str!("../svg/file-json.svg")),
    ("file-text", include_str!("../svg/file-text.svg")),
    ("file-style", include_str!("../svg/file-style.svg")),
    ("chevron-right", include_str!("../svg/chevron-right.svg")),
    ("chevron-down", include_str!("../svg/chevron-down.svg")),
    ("chevron-up", include_str!("../svg/chevron-up.svg")),
    ("x", include_str!("../svg/x.svg")),
    ("search", include_str!("../svg/search.svg")),
    ("plus", include_str!("../svg/plus.svg")),
    ("cpu", include_str!("../svg/cpu.svg")),
    ("memory", include_str!("../svg/memory.svg")),
    ("gpu", include_str!("../svg/gpu.svg")),
    ("git-branch", include_str!("../svg/git-branch.svg")),
    ("git-commit", include_str!("../svg/git-commit.svg")),
    ("indent", include_str!("../svg/indent.svg")),
    ("line-ending", include_str!("../svg/line-ending.svg")),
    ("encoding", include_str!("../svg/encoding.svg")),
    ("braces", include_str!("../svg/braces.svg")),
    ("arrow-left", include_str!("../svg/arrow-left.svg")),
    ("arrow-right", include_str!("../svg/arrow-right.svg")),
    ("chevron-left", include_str!("../svg/chevron-left.svg")),
    ("crosshair", include_str!("../svg/crosshair.svg")),
    ("chevrons-expand", include_str!("../svg/chevrons-expand.svg")),
    ("chevrons-collapse", include_str!("../svg/chevrons-collapse.svg")),
    ("more-vertical", include_str!("../svg/more-vertical.svg")),
    ("more-horizontal", include_str!("../svg/more-horizontal.svg")),
    ("minus", include_str!("../svg/minus.svg")),
    ("log-out", include_str!("../svg/log-out.svg")),
    ("copy-plus", include_str!("../svg/copy-plus.svg")),
    ("replace", include_str!("../svg/replace.svg")),
    ("folder-tool", include_str!("../svg/folder-tool.svg")),
    ("terminal", include_str!("../svg/terminal.svg")),
    ("view-split-down", include_str!("../svg/view-split-down.svg")),
    ("copy", include_str!("../svg/copy.svg")),
    ("scissors", include_str!("../svg/scissors.svg")),
    ("clipboard", include_str!("../svg/clipboard.svg")),
    ("hash", include_str!("../svg/hash.svg")),
    ("link", include_str!("../svg/link.svg")),
    ("app-window", include_str!("../svg/app-window.svg")),
    ("rotate-ccw", include_str!("../svg/rotate-ccw.svg")),
    ("x-square", include_str!("../svg/x-square.svg")),
    ("arrow-left-to-line", include_str!("../svg/arrow-left-to-line.svg")),
    ("arrow-right-to-line", include_str!("../svg/arrow-right-to-line.svg")),
    ("file-check", include_str!("../svg/file-check.svg")),
    ("layers-x", include_str!("../svg/layers-x.svg")),
    ("modules", include_str!("../svg/modules.svg")),
    ("view-split-move", include_str!("../svg/view-split-move.svg")),
    ("view-split-move-down", include_str!("../svg/view-split-move-down.svg")),
    ("compare", include_str!("../svg/compare.svg")),
    ("graph", include_str!("../svg/graph.svg")),
    ("arrow-up-right", include_str!("../svg/arrow-up-right.svg")),
    ("pipette", include_str!("../svg/pipette.svg")),
    ("keyboard", include_str!("../svg/keyboard.svg")),
    ("trash", include_str!("../svg/trash.svg")),
    ("arrow-up", include_str!("../svg/arrow-up.svg")),
    ("arrow-down", include_str!("../svg/arrow-down.svg")),
    ("clock", include_str!("../svg/clock.svg")),
    ("download", include_str!("../svg/download.svg")),
    ("folder-input", include_str!("../svg/folder-input.svg")),
    ("tag", include_str!("../svg/tag.svg")),
    ("eye", include_str!("../svg/eye.svg")),
    ("eye-off", include_str!("../svg/eye-off.svg")),
    ("server", include_str!("../svg/server.svg")),
    ("cloud", include_str!("../svg/cloud.svg")),
    ("square", include_str!("../svg/square.svg")),
    ("rows", include_str!("../svg/rows.svg")),
    ("upload", include_str!("../svg/upload.svg")),
    ("refresh", include_str!("../svg/refresh.svg")),
    ("user", include_str!("../svg/user.svg")),
    ("lock", include_str!("../svg/lock.svg")),
    ("lock-open", include_str!("../svg/lock-open.svg")),
    ("key", include_str!("../svg/key.svg")),
    ("maximize-2", include_str!("../svg/maximize-2.svg")),
    ("minimize-2", include_str!("../svg/minimize-2.svg")),
    ("filter", include_str!("../svg/filter.svg")),
    ("transpose", include_str!("../svg/transpose.svg")),
    ("table", include_str!("../svg/table.svg")),
    ("undo", include_str!("../svg/undo.svg")),
    ("check", include_str!("../svg/check.svg")),
    ("info", include_str!("../svg/info.svg")),
    ("circle-alert", include_str!("../svg/circle-alert.svg")),
    ("bell", include_str!("../svg/bell.svg")),
    ("database", include_str!("../svg/database.svg")),
    ("services", include_str!("../svg/services.svg")),
    ("text-wrap", include_str!("../svg/text-wrap.svg")),
    ("arrow-down-to-line", include_str!("../svg/arrow-down-to-line.svg")),
    ("layers", include_str!("../svg/layers.svg")),
    ("column", include_str!("../svg/column.svg")),
    ("index", include_str!("../svg/index.svg")),
    ("play", include_str!("../svg/play.svg")),
    ("pause", include_str!("../svg/pause.svg")),
    ("stop", include_str!("../svg/stop.svg")),
    ("gear", include_str!("../svg/gear.svg")),
    ("console", include_str!("../svg/console.svg")),
    ("comment", include_str!("../svg/comment.svg")),
    ("parameter", include_str!("../svg/parameter.svg")),
    ("plan", include_str!("../svg/plan.svg")),
    ("flame", include_str!("../svg/flame.svg")),
    ("octagon-alert", include_str!("../svg/octagon-alert.svg")),
    ("hard-drive", include_str!("../svg/hard-drive.svg")),
    ("lightbulb", include_str!("../svg/lightbulb.svg")),
    ("route", include_str!("../svg/route.svg")),
    ("panel-right", include_str!("../svg/panel-right.svg")),
    ("sparkles", include_str!("../svg/sparkles.svg")),
    ("text-cursor", include_str!("../svg/text-cursor.svg")),
    ("sun", include_str!("../svg/sun.svg")),
    ("moon", include_str!("../svg/moon.svg")),
    ("zap", include_str!("../svg/zap.svg")),
    ("droplet", include_str!("../svg/droplet.svg")),
    ("cloud-fog", include_str!("../svg/cloud-fog.svg")),
    ("butterfly", include_str!("../svg/butterfly.svg")),
    ("balloon", include_str!("../svg/balloon.svg")),
    ("heart", include_str!("../svg/heart.svg")),
    ("bat", include_str!("../svg/bat.svg")),
    ("lava-lamp", include_str!("../svg/lava-lamp.svg")),
    ("tv", include_str!("../svg/tv.svg")),
    ("glitch", include_str!("../svg/glitch.svg")),
    ("cloud-rain", include_str!("../svg/cloud-rain.svg")),
    ("holo-sign", include_str!("../svg/holo-sign.svg")),
    ("menu", include_str!("../svg/menu.svg")),
    ("compass", include_str!("../svg/compass.svg")),
    ("wand", include_str!("../svg/wand.svg")),
    ("redo", include_str!("../svg/redo.svg")),
    ("wrench", include_str!("../svg/wrench.svg")),
    ("circle-help", include_str!("../svg/circle-help.svg")),
];

/// Identifier of a registered icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconId(u16);

/// `true` if the glyph key belongs to an icon (not to text).
pub fn is_icon(key: GlyphKey) -> bool {
    key.0 & FLAG != 0
}

struct Icon {
    name: String,
    source: String,
    /// Parsed per look on first use (`None`: invalid SVG); looks no longer shown are
    /// dropped ([`Icons::set_look`], [`Icons::release_unused`]).
    trees: Mutex<Vec<(IconLook, Option<Arc<usvg::Tree>>)>>,
}

/// The icon set. Shared between the UI (which requests keys) and the renderer (which
/// rasterizes them), hence `Sync`.
#[derive(Default)]
pub struct Icons {
    icons: Vec<Icon>,
    by_name: HashMap<String, IconId>,
    /// The packs registered (by modules), in their order.
    packs: Vec<Arc<dyn IconPack>>,
    /// The file icon sets registered, the one chosen (its place + 1; 0: ours, by
    /// language), and their icons asked so far.
    file_sets: Vec<Arc<dyn FileIconSet>>,
    file_set: AtomicU8,
    colored: Mutex<Vec<Colored>>,
    /// The look drawn ([`IconLook`]'s code), shared by every window using the set.
    look: AtomicU8,
}

impl std::fmt::Debug for Icons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icons").field("count", &self.icons.len()).finish()
    }
}

impl Icons {
    /// The bundled set.
    pub fn builtin() -> Self {
        let mut icons = Self::default();
        for (name, svg) in BUILTIN {
            icons.register(name, svg);
        }
        icons
    }

    /// Registers (or replaces) an icon. Also used by modules.
    pub fn register(&mut self, name: &str, svg: &str) -> IconId {
        let icon =
            Icon { name: name.to_owned(), source: svg.to_owned(), trees: Mutex::new(Vec::new()) };
        if let Some(&id) = self.by_name.get(name) {
            self.icons[id.0 as usize] = icon;
            return id;
        }
        let id = IconId(self.icons.len() as u16);
        self.icons.push(icon);
        self.by_name.insert(name.to_owned(), id);
        id
    }

    pub fn id(&self, name: &str) -> Option<IconId> {
        self.by_name.get(name).copied()
    }

    /// The icon `name`, or the generic file icon if it doesn't exist.
    pub fn id_or_file(&self, name: &str) -> IconId {
        self.id(name).or_else(|| self.id("file")).unwrap_or(IconId(0))
    }

    pub fn name(&self, id: IconId) -> Option<&str> {
        self.icons.get(id.0 as usize).map(|i| i.name.as_str())
    }

    /// Registers a pack (a module's): it can then be chosen ([`Self::set_look`]).
    pub fn register_pack(&mut self, pack: Arc<dyn IconPack>) -> IconLook {
        if let Some(i) = self.packs.iter().position(|p| p.id() == pack.id()) {
            self.packs[i] = pack;
            return IconLook::Pack(i as u8);
        }
        self.packs.push(pack);
        IconLook::Pack((self.packs.len() - 1) as u8)
    }

    /// Registers a file icon set (a module's): it can then be chosen
    /// ([`Self::set_file_set`]). Its place.
    pub fn register_file_set(&mut self, set: Arc<dyn FileIconSet>) -> u8 {
        if let Some(i) = self.file_sets.iter().position(|s| s.id() == set.id()) {
            self.file_sets[i] = set;
            return i as u8;
        }
        self.file_sets.push(set);
        (self.file_sets.len() - 1) as u8
    }

    pub fn file_sets(&self) -> &[Arc<dyn FileIconSet>] {
        &self.file_sets
    }

    /// The file icon set drawn (its place; `None`: ours, by language).
    pub fn file_set(&self) -> Option<u8> {
        self.file_set.load(Ordering::Relaxed).checked_sub(1)
    }

    /// Files drawn with set `set` from now on (`None`: ours); the one before lets its data
    /// and its drawings go.
    pub fn set_file_set(&self, set: Option<u8>) {
        let before = self.file_set();
        self.file_set.store(set.map_or(0, |s| s + 1), Ordering::Relaxed);
        if before != set
            && let Some(old) = before
        {
            if let Some(s) = self.file_sets.get(usize::from(old)) {
                s.release();
            }
            if let Ok(colored) = self.colored.lock() {
                for c in colored.iter().filter(|c| c.set == old) {
                    if let Ok(mut tree) = c.tree.lock() {
                        *tree = None;
                    }
                }
            }
        }
    }

    /// The chosen file icon set's icon for file `name` (its language: `language`), if it has
    /// one — drawn in its own colors; `None`: ours (by language).
    pub fn file_icon(&self, name: &str, language: &str) -> Option<IconId> {
        self.file_icon_in(self.file_set()?, name, language)
    }

    /// The chosen file icon set's icon for folder `name`, open or not, if it has one.
    pub fn folder_icon(&self, name: &str, open: bool) -> Option<IconId> {
        self.folder_icon_in(self.file_set()?, name, open)
    }

    /// Set `set`'s icon for file `name` (e.g. a preview in the settings).
    pub fn file_icon_in(&self, set: u8, name: &str, language: &str) -> Option<IconId> {
        let key = self.file_sets.get(usize::from(set))?.file(name, language)?;
        self.colored_id(set, key)
    }

    /// Set `set`'s icon for folder `name`, open or not.
    pub fn folder_icon_in(&self, set: u8, name: &str, open: bool) -> Option<IconId> {
        let key = self.file_sets.get(usize::from(set))?.folder(name, open)?;
        self.colored_id(set, key)
    }

    /// A file's icon — the one place that decides it: the chosen file icon set's when it
    /// has one (its own colors), else ours (`ours`: our icon's name for it, tinted by its
    /// language). Modules only name the file.
    pub fn file(&self, name: &str, ours: &str, language: &str) -> (IconId, Tint) {
        match self.file_icon(name, language) {
            Some(id) => (id, Tint::Own),
            None => (self.id_or_file(ours), Tint::Language(language.to_owned())),
        }
    }

    /// A folder's icon: the chosen set's (its own colors), else ours (the folder's tint).
    pub fn folder(&self, name: &str, open: bool) -> (IconId, Tint) {
        match self.folder_icon(name, open) {
            Some(id) => (id, Tint::Own),
            None => (self.id_or_file(if open { "folder-open" } else { "folder" }), Tint::Folder),
        }
    }

    /// Whether icon `id` is drawn in its own colors (a file icon set's).
    pub fn is_colored(id: IconId) -> bool {
        id.0 & COLORED != 0
    }

    fn colored_id(&self, set: u8, key: &'static str) -> Option<IconId> {
        let mut colored = self.colored.lock().ok()?;
        if let Some(i) = colored.iter().position(|c| c.set == set && c.key == key) {
            return Some(IconId(COLORED | i as u16));
        }
        if colored.len() >= usize::from(COLORED) - 1 {
            return None;
        }
        colored.push(Colored { set, key, tree: Mutex::new(None) });
        Some(IconId(COLORED | (colored.len() - 1) as u16))
    }

    /// Rasterizes colored icon `index` at `size`: its own colors (straight RGBA).
    fn rasterize_colored(&self, index: usize, size: u32) -> Option<RasterizedGlyph> {
        let tree = {
            let colored = self.colored.lock().ok()?;
            let icon = colored.get(index)?;
            let mut tree = icon.tree.lock().ok()?;
            if tree.is_none() {
                let set = self.file_sets.get(usize::from(icon.set))?;
                let parsed = set
                    .svg(icon.key)
                    .and_then(|svg| usvg::Tree::from_str(&svg, &usvg::Options::default()).ok())
                    .map(Arc::new);
                *tree = Some(parsed);
            }
            tree.clone().flatten()
        }?;
        let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
        let svg = tree.size();
        let scale = size as f32 / svg.width().max(svg.height());
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let data = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        Some(RasterizedGlyph {
            left: 0,
            top: size as i32,
            width: size,
            height: size,
            format: GlyphFormat::Color,
            data,
        })
    }

    /// Every look that can be chosen: our styles, then the packs registered.
    pub fn looks(&self) -> Vec<IconLook> {
        let styles = IconStyle::ALL.into_iter().map(IconLook::Style);
        styles.chain((0..self.packs.len()).map(|i| IconLook::Pack(i as u8))).collect()
    }

    /// Look `look`'s pack, if it's one.
    pub fn pack(&self, look: IconLook) -> Option<&Arc<dyn IconPack>> {
        match look {
            IconLook::Pack(i) => self.packs.get(usize::from(i)),
            IconLook::Style(_) => None,
        }
    }

    /// The id a look is saved as (a style's, a pack's).
    pub fn look_id(&self, look: IconLook) -> String {
        match look {
            IconLook::Style(style) => style.id().to_owned(),
            IconLook::Pack(_) => self.pack(look).map_or_else(String::new, |p| p.id().to_owned()),
        }
    }

    /// The look saved as `id` (a style's, an old style id too, or a registered pack's).
    pub fn look_from_id(&self, id: &str) -> Option<IconLook> {
        if let Some(style) = IconStyle::from_id(id) {
            return Some(IconLook::Style(style));
        }
        self.packs.iter().position(|p| p.id() == id).map(|i| IconLook::Pack(i as u8))
    }

    /// The look drawn.
    pub fn look(&self) -> IconLook {
        IconLook::from_code(self.look.load(Ordering::Relaxed))
    }

    /// The style drawn (ours; a pack's look counts as the regular style).
    pub fn style(&self) -> IconStyle {
        match self.look() {
            IconLook::Style(style) => style,
            IconLook::Pack(_) => IconStyle::default(),
        }
    }

    /// Draws the icons in `look` from now on (the other looks' shapes are released, a pack
    /// no longer drawn lets its data go).
    pub fn set_look(&self, look: IconLook) {
        let before = self.look();
        self.look.store(look.code(), Ordering::Relaxed);
        if before != look
            && let Some(pack) = self.pack(before)
        {
            pack.release();
        }
        self.release_unused();
    }

    /// Draws the icons in our `style` from now on.
    pub fn set_style(&self, style: IconStyle) {
        self.set_look(IconLook::Style(style));
    }

    /// The drawings read from the icons' SVG let go (the window idle): they're needed only
    /// to rasterize an icon the atlas doesn't hold (a new size, the atlas cleared), and read
    /// again then.
    pub fn release_drawings(&self) {
        for icon in &self.icons {
            if let Ok(mut trees) = icon.trees.lock() {
                *trees = Vec::new();
            }
        }
        if let Ok(colored) = self.colored.lock() {
            for icon in colored.iter() {
                if let Ok(mut tree) = icon.tree.lock() {
                    *tree = None;
                }
            }
        }
    }

    /// Releases the shapes of the looks not drawn (e.g. after the settings' previews).
    pub fn release_unused(&self) {
        let look = self.look();
        for icon in &self.icons {
            if let Ok(mut trees) = icon.trees.lock() {
                trees.retain(|(l, _)| *l == look);
            }
        }
        // The packs previewed but not drawn let their data go.
        for (i, pack) in self.packs.iter().enumerate() {
            if look != IconLook::Pack(i as u8) {
                pack.release();
            }
        }
    }

    /// Glyph key for icon `id` at `size` physical pixels, in the current look.
    pub fn key(&self, id: IconId, size: u16) -> GlyphKey {
        self.key_in(id, size, self.look())
    }

    /// Glyph key for icon `id` in our `style` (e.g. a preview in the settings).
    pub fn key_styled(&self, id: IconId, size: u16, style: IconStyle) -> GlyphKey {
        self.key_in(id, size, IconLook::Style(style))
    }

    /// Glyph key for icon `id` in `look` (e.g. a pack's preview in the settings).
    pub fn key_in(&self, id: IconId, size: u16, look: IconLook) -> GlyphKey {
        GlyphKey(FLAG | (u64::from(look.code()) << 40) | (u64::from(id.0) << 16) | u64::from(size))
    }

    /// Rasterizes the icon requested by key: a `size`×`size` mask with its top-left corner
    /// at `(origin.x, origin.y - size)` (like a glyph sitting on the baseline).
    pub fn rasterize_key(&self, key: GlyphKey) -> Option<RasterizedGlyph> {
        if !is_icon(key) {
            return None;
        }
        let id = ((key.0 >> 16) & 0xffff) as usize;
        let size = (key.0 & 0xffff) as u32;
        if id & usize::from(COLORED) != 0 {
            return self.rasterize_colored(id & !usize::from(COLORED), size);
        }
        let look = IconLook::from_code(((key.0 >> 40) & 0xff) as u8);
        let icon = self.icons.get(id)?;
        let tree = {
            let mut trees = icon.trees.lock().ok()?;
            match trees.iter().find(|(l, _)| *l == look) {
                Some((_, tree)) => tree.clone(),
                None => {
                    // A pack's drawing when it has one; else ours (never missing).
                    let source = match look {
                        IconLook::Style(style) => style.restyle(&icon.source),
                        IconLook::Pack(_) => self
                            .pack(look)
                            .and_then(|p| p.svg(&icon.name))
                            .map_or_else(|| icon.source.clone(), |svg| svg.into_owned()),
                    };
                    let tree =
                        usvg::Tree::from_str(&source, &usvg::Options::default()).ok().map(Arc::new);
                    trees.push((look, tree.clone()));
                    tree
                }
            }
        }?;
        let tree = tree.as_ref();
        let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
        let svg = tree.size();
        let scale = size as f32 / svg.width().max(svg.height());
        resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let data = pixmap.pixels().iter().map(|p| p.alpha()).collect();
        Some(RasterizedGlyph {
            left: 0,
            top: size as i32,
            width: size,
            height: size,
            format: GlyphFormat::Mask,
            data,
        })
    }
}

/// Combines text and icon rasterization, for the renderer.
pub struct WithIcons<'a, T: GlyphRasterizer> {
    pub text: &'a mut T,
    pub icons: &'a Arc<Icons>,
}

impl<T: GlyphRasterizer> std::fmt::Debug for WithIcons<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WithIcons")
    }
}

impl<T: GlyphRasterizer> GlyphRasterizer for WithIcons<'_, T> {
    fn rasterize(&mut self, key: GlyphKey) -> Option<RasterizedGlyph> {
        if is_icon(key) { self.icons.rasterize_key(key) } else { self.text.rasterize(key) }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
