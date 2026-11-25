//! The settings window (⌘,): a [`Modal`] with the sections on the left ("Appearance", "Language") and
//! the section content on the right, built from the design system components (theme
//! cards with preview, toggle, choice pills).
//!
//! The component only displays and reports actions ([`SettingsAction`]); applying and
//! saving are done by the app.

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::TextSystem;
use ui_components::control::{Control, Controls, Shown, Thumb};
use ui_components::search_field::SearchField;
use ui_kit::{KeyHint, draw_text};
use workbench_model::{SettingsGroup, SettingsPage, SettingsRow};

use crate::ds::{self, Ctx};
use crate::modal::{Modal, ModalHit, ModalSection, ModalSize, ModalSpec};
use crate::palette::Palette;

mod appearance;
mod cards;
mod input;
mod keymap;
mod model;
mod modules;
mod paint;
mod rows;
mod search;
mod text;

pub use model::{
    ContributedBlock, FileIconChoice, FrameworkRow, IconChoice, ModuleRow, SettingsAction,
    SettingsModel, ThemeOption, ThemePreview,
};

/// A block's text field being written: its block, its row, the text so far.
#[derive(Debug)]
struct Editing {
    block: &'static str,
    key: &'static str,
    input: ui_kit::TextInput,
}

/// Which themes the Appearance section lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeFilter {
    #[default]
    All,
    Dark,
    Light,
}

impl ThemeFilter {
    const ALL: [Self; 3] = [Self::All, Self::Dark, Self::Light];

    fn label(self) -> String {
        i18n::t(match self {
            Self::All => "settings.themes_all",
            Self::Dark => "settings.themes_dark",
            Self::Light => "settings.themes_light",
        })
    }

    fn keeps(self, theme: &ThemeOption) -> bool {
        match self {
            Self::All => true,
            Self::Dark => theme.dark,
            Self::Light => !theme.dark,
        }
    }
}

/// Cards side by side in the settings' grids (themes, icon sets).
const GRID_COLUMNS: usize = 3;

const APPEARANCE: &str = "appearance";
const LANGUAGE: &str = "language";
const KEYMAP: &str = "keymap";
const MODULES: &str = "modules";
const ICONS: &str = "icons";

/// The app's own pages (what they show is the window's): where they go.
const APP_PAGES: [SettingsPage; 5] = [
    SettingsPage {
        id: APPEARANCE,
        label: "settings.appearance",
        icon: "palette",
        group: SettingsGroup::Interface,
        order: 10,
    },
    SettingsPage {
        id: ICONS,
        label: "settings.icons",
        icon: "folder",
        group: SettingsGroup::Interface,
        order: 20,
    },
    SettingsPage {
        id: KEYMAP,
        label: "settings.keymap",
        icon: "keyboard",
        group: SettingsGroup::Application,
        order: 10,
    },
    SettingsPage {
        id: MODULES,
        label: "settings.modules",
        icon: "modules",
        group: SettingsGroup::Application,
        order: 20,
    },
    SettingsPage {
        id: LANGUAGE,
        label: "settings.language",
        icon: "globe",
        group: SettingsGroup::Application,
        order: 30,
    },
];

/// The settings' search field in the header (logical pixels wide).
const SEARCH_FIELD_W: f32 = 280.0;

/// Where a settings section is painted: the layer, the text, the drawing context and the
/// content column (its left edge, top and width).
struct SectionArea<'a, 'c> {
    layer: &'a mut Layer,
    text: &'a mut TextSystem,
    ctx: &'a Ctx<'c>,
    x0: f32,
    top: f32,
    width: f32,
}

/// A dropdown row, as painted: its field, its block and key, its choices and the current one.
#[derive(Debug, Clone, PartialEq)]
struct Dropdown {
    field: Rect,
    block: &'static str,
    key: &'static str,
    choices: Vec<(i64, String)>,
    current: i64,
}

/// The two search fields' places among the settings' controls (see `with_controls`).
const QUERY: usize = 3;
const FILTER: usize = 4;

/// What wakes the app (the window's waker).
#[derive(Clone)]
struct Wake(std::sync::Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wake")
    }
}

#[derive(Debug, Default)]
pub struct SettingsView {
    modal: Modal,
    /// What a search looks through, made for the model's shape ([`entries_shape`]): not again
    /// at each frame while a query is typed (hundreds of texts, every language's words).
    entries: Option<(u64, std::sync::Arc<Vec<Entry>>)>,
    /// The content's scrollbar (in the content's right margin: never over it), and where the
    /// mouse last was (the wheel scrolls what's under it: the navigation or the content).
    bar: ui_components::EdgeScrollbar,
    mouse: Option<Point>,
    section: Option<&'static str>,
    cards: Vec<(Rect, String)>,
    /// The themes shown (All / Dark / Light), its segments (from the last paint) and the
    /// one under the mouse.
    theme_filter: ThemeFilter,
    filter_rects: Vec<Rect>,
    filter_hover: Option<usize>,
    /// The keymap preset chips and the shortcut rows (from the last paint).
    keymap_chips: Vec<(Rect, String)>,
    binding_rows: Vec<Rect>,
    /// Settings → Shortcuts: the commands and their keys.
    shortcuts: crate::shortcuts_editor::ShortcutsEditor,
    toggle: Rect,
    /// The "Create a theme…" button.
    studio: Rect,
    languages: Vec<(Rect, String)>,
    /// The icon set cards (from the last paint).
    icon_cards: Vec<(Rect, icons::IconLook)>,
    file_icon_cards: Vec<(Rect, Option<u8>)>,
    /// The framework rows (clickable) and the core module rows (from the last paint).
    framework_rows: Vec<(Rect, String)>,
    module_rows: Vec<Rect>,
    /// The modules' switches, checkboxes and the loading choice (from the last paint).
    module_hits: Vec<(Rect, SettingsAction)>,
    /// The modules' blocks: their choices and buttons, and where the last one ends.
    block_hits: Vec<(Rect, SettingsAction)>,
    /// The dropdown rows painted: their fields and choices.
    dropdowns: Vec<Dropdown>,
    /// A dropdown's list, open: the shared picker, and the row it's for.
    picker: ui_components::picker::Picker,
    picker_for: Option<Dropdown>,
    /// Its controls' state of use (see [`Self::with_controls`]).
    controls: Controls,
    /// The window's area (a dropdown's list stays within it).
    viewport: Rect,
    blocks_bottom: f32,
    /// Each row's area (its block, its key): what a search result leads to.
    row_anchors: Vec<(&'static str, &'static str, Rect)>,
    /// Scrolled to the content's end at the next paint (once its height is known).
    scroll_to_end: bool,
    /// The pages whose "Advanced" is open (closed when the window opens).
    advanced_open: Vec<&'static str>,
    /// A search result opened belongs to another language of the Languages page: the app
    /// chooses it ([`SettingsView::take_wanted_language`]).
    wanted_language: Option<String>,
    /// The blocks' sliders (where, their block and row, each stop's value), the one held
    /// (dragged) and the last value it gave.
    sliders: Vec<(ui_components::SliderArea, &'static str, &'static str, Vec<i64>)>,
    slider_held: Option<(usize, i64)>,
    /// The blocks' text fields (where, their block and row, the value saved), and the one
    /// being written (it has the keys until Enter, Esc or a click elsewhere).
    text_fields: Vec<(Rect, &'static str, &'static str, String)>,
    /// The blocks' secret fields among them (their block and row): written masked, from empty.
    secret_fields: Vec<(&'static str, &'static str)>,
    editing: Option<Editing>,
    /// The sections in the navigation's order (the app's, and the modules' pages; from the
    /// last paint).
    section_ids: Vec<&'static str>,
    hover: Option<SettingsAction>,
    /// The content scroll (physical pixels) and its area.
    scroll: f32,
    /// The content's height, measured at the last paint (see `clamp_scroll`).
    content_height: f32,
    content: Rect,
    /// Settings search: the text, the results (indices into entries), the selection, their areas.
    /// The settings' search (the app's search field: ×, ⌘A; its focus the controls').
    query: SearchField,
    results: Vec<usize>,
    selected: usize,
    result_rects: Vec<Rect>,
    /// The found setting, briefly highlighted after opening (the anchor and the fade-out).
    /// The element a search result led to (anchor, exact target) and its highlight.
    flash: Option<(&'static str, Option<String>, crate::anim::Tween)>,
    /// The flashed element must be scrolled into view (on the next paint).
    reveal: bool,
    /// The areas of the drawn settings (for highlighting): (anchor, area).
    anchors: Vec<(&'static str, Rect)>,
    /// The semantic model (if active) and the entry vectors (computed once per entries).
    embedder: Option<SharedEmbedder>,
    vectors: Vec<Vec<f32>>,
    /// The entries' fingerprint `vectors` were made for.
    vectors_for: Option<u64>,
    /// The vectors being made in the background (for that fingerprint): the search finds
    /// by words meanwhile, and again by meaning once they're here.
    making: Option<(u64, std::sync::mpsc::Receiver<Vec<Vec<f32>>>)>,
    /// Wakes the app when the vectors are made (no frames asked while they're on their way).
    waker: Option<Wake>,
    /// What `results` were found for: the folded query and the entries' fingerprint (the
    /// search runs again only when either changes, not on every frame).
    searched: Option<(String, u64)>,
    /// The cards' pictures (the modules' previews).
    pictures: cards::CardPictures,
    /// Each block's description broken in lines, kept while its text, width and scale stay
    /// (the window repaints on every hover: not broken again each time); let go when the
    /// window closes and by "Clear and Rebuild Caches" ([`kernel::Purge`]).
    block_details: std::collections::HashMap<&'static str, BlockDetail>,
}

/// A block's description broken in lines, for its text, the width and the scale.
type BlockDetail = kernel::cache::Memo<(String, u32, u32), Vec<String>>;

/// The embeddings model (without its own `Debug`).
#[derive(Clone)]
struct SharedEmbedder(std::sync::Arc<dyn text_search::Embedder>);

impl std::fmt::Debug for SharedEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Embedder")
    }
}

/// A searchable setting: a block, a row of it (its key the target), a theme card…
#[derive(Debug, Default)]
struct Entry {
    section: &'static str,
    anchor: &'static str,
    /// The exact element within the anchor (e.g. a theme's id → its card).
    target: Option<String>,
    title: String,
    detail: String,
    keywords: String,
    /// The language it's about (the Languages page then shows it), and whether it's under
    /// its page's "Advanced" (opened then).
    language: Option<String>,
    advanced: bool,
}

/// The navigation's pages: the app's, and those the blocks are on (described by one of
/// their blocks; a page nobody describes goes last, named by its first block) — in their
/// groups' order, then their places: never the order the modules came in. A page without
/// blocks (its module off) isn't there.
pub(crate) fn pages(blocks: &[ContributedBlock]) -> Vec<NavPage> {
    let mut out: Vec<NavPage> =
        APP_PAGES.iter().map(|p| NavPage { page: *p, label: i18n::t(p.label) }).collect();
    for b in blocks {
        if out.iter().any(|p| p.page.id == b.page) {
            continue;
        }
        let described = blocks.iter().find_map(|o| o.page_info.filter(|p| p.id == b.page));
        out.push(match described {
            Some(page) => NavPage { page, label: i18n::t(page.label) },
            None => NavPage {
                page: SettingsPage {
                    id: b.page,
                    label: "",
                    icon: "file",
                    group: SettingsGroup::Tools,
                    order: u16::MAX,
                },
                label: b.block.title.clone(),
            },
        });
    }
    out.sort_by_key(|p| p.page.sort_key());
    out
}

/// The blocks in the order the window shows them: by page, those under "Advanced" last,
/// then by their places and ids — whatever order the modules registered in.
pub fn order_blocks(blocks: &mut [ContributedBlock]) {
    blocks.sort_by_key(|b| (b.page, b.advanced, b.order, b.id));
}

/// A page as the navigation shows it: where it goes and its name.
#[derive(Debug, Clone)]
pub(crate) struct NavPage {
    page: SettingsPage,
    label: String,
}

/// The pages' sections for the modal (their groups' captions from `groups`).
fn modal_sections<'a>(pages: &'a [NavPage], groups: &'a [String; 4]) -> Vec<ModalSection<'a>> {
    pages
        .iter()
        .map(|p| ModalSection {
            id: p.page.id,
            label: &p.label,
            icon: p.page.icon,
            group: &groups[p.page.group as usize],
            group_icon: p.page.group.icon(),
        })
        .collect()
}

/// The groups' captions, in [`SettingsGroup`]'s order.
fn group_labels() -> [String; 4] {
    SettingsGroup::ALL.map(|g| i18n::t(g.label()))
}

/// "Clear and Rebuild Caches": what it keeps only to avoid recomputing (the blocks'
/// descriptions broken in lines) goes, made again at the next paint.
impl kernel::Purge for SettingsView {
    fn purge(&mut self) {
        self.block_details = std::collections::HashMap::new();
    }
}

impl SettingsView {
    pub fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    pub fn open(&mut self) {
        self.modal.open();
        self.advanced_open.clear();
        self.hover = None;
        // Opened: no field has the keys yet (typing searches the settings).
        self.controls = Controls::default();
        self.editing = None;
    }

    pub fn close(&mut self) {
        self.modal.close();
        self.hover = None;
        self.editing = None;
        // The cards' pictures: what their previews made goes.
        self.release_cards();
        // The semantic model and the entries' vectors: given again at the next opening, not
        // held meanwhile (the model is let go when nothing else uses it).
        self.set_embedder(None);
        // The blocks' descriptions: broken again at the next opening.
        self.block_details = std::collections::HashMap::new();
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.modal.set_viewport(viewport, scale);
    }

    /// Something moves: its window appearing, a setting found flashing. The vectors being
    /// made aren't: they wake the app when they're here ([`Self::set_waker`]).
    pub fn is_animating(&self) -> bool {
        self.modal.is_animating() || self.flash.as_ref().is_some_and(|(_, _, t)| t.animating())
    }

    /// What wakes the app when work done in the background (the search's vectors) is ready.
    pub fn set_waker(&mut self, waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
        self.waker = waker.map(Wake);
    }

    /// The page `id` shown (as a click on it; nothing if there's none).
    pub fn show_page(&mut self, id: &str) {
        if let Some(&page) = self.section_ids.iter().find(|p| **p == id) {
            self.section = Some(page);
            self.scroll = 0.0;
        }
    }

    /// Every page's "Advanced" open, the page scrolled down to it (its end).
    pub fn open_advanced(&mut self) {
        self.scroll_to_end = true;
        for &page in &self.section_ids {
            if !self.advanced_open.contains(&page) {
                self.advanced_open.push(page);
            }
        }
    }

    /// The language a search result opened asks for (once).
    pub fn take_wanted_language(&mut self) -> Option<String> {
        self.wanted_language.take()
    }

    /// A page's "Advanced" opened or closed.
    fn toggle_advanced(&mut self, page: &'static str) {
        match self.advanced_open.iter().position(|p| *p == page) {
            Some(i) => {
                self.advanced_open.remove(i);
            }
            None => self.advanced_open.push(page),
        }
    }

    fn active(&self) -> &'static str {
        self.section.unwrap_or(APPEARANCE)
    }

    /// The semantic model for settings search (if active).
    pub fn set_embedder(&mut self, embedder: Option<std::sync::Arc<dyn text_search::Embedder>>) {
        if embedder.is_none() != self.embedder.is_none() {
            self.vectors = Vec::new();
            self.vectors_for = None;
            self.making = None;
            self.searched = None;
        }
        self.embedder = embedder.map(SharedEmbedder);
    }

    /// The sections the navigation lists for `model` (the modules' pages among them): known
    /// before the first paint, so ↑ / ↓ work at once.
    pub fn set_sections(&mut self, model: &SettingsModel) {
        self.section_ids = pages(&model.blocks).iter().map(|p| p.page.id).collect();
    }

    /// ↑ / ↓: the previous / next section. `true` if it changed.
    pub fn step_section(&mut self, delta: isize) -> bool {
        let ids = &self.section_ids;
        let none = String::new();
        let list: Vec<ModalSection<'_>> = ids
            .iter()
            .map(|&id| ModalSection { id, label: &none, icon: "", group: &none, group_icon: "" })
            .collect();
        let next = self.modal.step_section(&list, self.active(), delta);
        let changed = next.is_some_and(|n| n != self.active());
        if let Some(next) = next {
            self.section = Some(next);
            self.scroll = 0.0;
        }
        changed
    }

    /// Opens the result chosen with the mouse (the `OpenResult` action).
    pub fn open_selected(&mut self, model: &SettingsModel) {
        self.confirm(model);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/settings_view.rs"]
mod tests;
