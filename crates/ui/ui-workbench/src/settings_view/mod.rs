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
use ui_kit::{KeyHint, draw_text};
use workbench_model::{SettingsGroup, SettingsPage, SettingsRow};

use crate::ds::{self, Ctx};
use crate::modal::{Modal, ModalHit, ModalSection, ModalSize, ModalSpec};
use crate::palette::Palette;

mod appearance;
mod cards;
mod keymap;
mod modules;
mod rows;
mod search;

/// A theme's colors for the preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePreview {
    pub window: Rgba,
    pub editor: Rgba,
    pub text: Rgba,
    pub accent: Rgba,
    pub keyword: Rgba,
    pub string: Rgba,
}

impl ThemePreview {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            window: p.window_background,
            editor: p.editor_background,
            text: p.code_text,
            accent: p.accent,
            keyword: p.terminal_ansi[3],
            string: p.terminal_ansi[2],
        }
    }
}

/// A theme in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeOption {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub preview: ThemePreview,
}

/// What the settings display (the current state, from the app).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsModel {
    pub themes: Vec<ThemeOption>,
    pub current_theme: String,
    pub follow_system: bool,
    /// The interface languages: (code, name in its own language).
    pub languages: Vec<(String, String)>,
    pub current_language: String,
    /// The icons' look chosen, and every one to choose from (our styles, the packs).
    pub icon_look: icons::IconLook,
    pub icon_looks: Vec<IconChoice>,
    /// The file icons chosen (`None`: ours, by language), and the sets to choose from.
    pub file_icon_set: Option<u8>,
    pub file_icon_sets: Vec<FileIconChoice>,
    /// Keymap presets: (id, name), the active one, and its shortcuts (command, keys as
    /// the system shows them).
    pub keymaps: Vec<(String, String)>,
    pub current_keymap: String,
    /// Every command, its keys (Settings → Shortcuts).
    pub bindings: std::sync::Arc<Vec<crate::shortcuts_editor::BindingRow>>,
    /// Every module (but the frameworks'): on / off, loaded at startup, in memory.
    pub modules: Vec<ModuleRow>,
    /// The modules' data is read at startup (else on first use, let go when unused).
    pub modules_at_startup: bool,
    /// The open project's name (`None`: no project, no framework choices).
    pub project: Option<String>,
    pub frameworks: Vec<FrameworkRow>,
    /// The modules' blocks (their own settings), in the order they're shown.
    pub blocks: Vec<ContributedBlock>,
    /// The Languages page's chooser: (id, name) of each language its blocks have something
    /// for, and the one shown.
    pub code_languages: Vec<(String, String)>,
    pub code_language: Option<String>,
}

/// A module's block in the settings (from its `SettingsContribution`): where it goes, the
/// words a search finds it by, and what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedBlock {
    pub id: &'static str,
    /// The page it's on.
    pub page: &'static str,
    /// The page it's on, described (its own, a shared one), if it describes it.
    pub page_info: Option<SettingsPage>,
    /// Its place on the page (lower first, then by id), and whether it's under the page's
    /// "Advanced".
    pub order: u16,
    pub advanced: bool,
    /// The one language it's about (shown on the Languages page only while it's chosen).
    pub language: Option<String>,
    /// Words a search finds it by, besides its title and detail (every language's).
    pub keywords: String,
    pub block: workbench_model::SettingsBlock,
}

/// An icon look to choose: itself, its name and what it is (a pack's author and license).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconChoice {
    pub look: icons::IconLook,
    pub name: String,
    pub detail: String,
}

/// A file icon set to choose (`None`: ours), its name and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIconChoice {
    pub set: Option<u8>,
    pub name: String,
    pub detail: String,
}

/// A module in the settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleRow {
    pub id: String,
    pub name: String,
    pub description: String,
    /// On (from the next start).
    pub enabled: bool,
    /// Working now.
    pub active: bool,
    /// Its data read at startup.
    pub preload: bool,
    /// Its data is in memory now.
    pub loaded: bool,
    /// The modules on that need it (it can't be turned off while they are).
    pub required_by: Vec<String>,
    /// Hidden (from the next start): it runs, but shows nothing of its own.
    pub hidden: bool,
    /// Running hidden now.
    pub hidden_now: bool,
}

/// A framework in the settings: whether the project uses it and why.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameworkRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub on: bool,
    /// Where it was detected from (e.g. `composer.json`).
    pub detected: Option<String>,
    /// Chosen in the project's settings (overriding the detection).
    pub chosen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SelectTheme(String),
    ToggleFollowSystem,
    SelectLanguage(String),
    SelectKeymap(String),
    /// Commands' shortcuts changed (each one's whole list; `None`: the preset's).
    Shortcuts(crate::shortcuts_editor::ShortcutChanges),
    SelectIcons(icons::IconLook),
    /// The file icons: a set's (its place), or ours (`None`).
    SelectFileIcons(Option<u8>),
    /// A settings search result (opening it is done by the component).
    OpenResult,
    /// The theme configurator.
    OpenThemeStudio,
    /// A framework turned on / off for the open project.
    ToggleFramework(String),
    /// A module turned on / off (from the next start).
    ToggleModule(String),
    /// A module hidden / shown (from the next start).
    ToggleModuleHidden(String),
    /// A module's data read at startup, or not.
    TogglePreload(String),
    /// The modules' data read at startup (`true`) or on first use.
    SetModulesAtStartup(bool),
    /// A module block's choice: its row's key and the value chosen.
    Choose {
        block: &'static str,
        key: &'static str,
        value: i64,
    },
    /// Another language chosen on the Languages page.
    SelectCodeLanguage(String),
    /// A row's "↺": its default chosen (as [`Self::Choose`]).
    Reset {
        block: &'static str,
        key: &'static str,
        value: i64,
    },
    /// A page's "Advanced" opened or closed (done by the component).
    ToggleAdvanced(&'static str),
    /// A module block's button (its action).
    Press {
        block: &'static str,
        action: &'static str,
    },
    /// A module block's text field, written: its row's key and the text.
    SetText {
        block: &'static str,
        key: &'static str,
        value: String,
    },
    Close,
}

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
    /// The search field's × button (with text) and its hover.
    clear: Option<Rect>,
    /// The search field (from the last paint) and whether it has focus: a click on it or
    /// typing focuses it; any other click in the settings takes the focus away.
    field: Rect,
    query_focused: bool,
    clear_hover: bool,
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
    query: ui_kit::TextInput,
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
    /// What `results` were found for: the folded query and the entries' fingerprint (the
    /// search runs again only when either changes, not on every frame).
    searched: Option<(String, u64)>,
    /// The cards' pictures (the modules' previews).
    pictures: cards::CardPictures,
}

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

impl SettingsView {
    pub fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    pub fn open(&mut self) {
        self.modal.open();
        self.advanced_open.clear();
        self.hover = None;
        self.query_focused = false;
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
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.modal.set_viewport(viewport, scale);
    }

    pub fn is_animating(&self) -> bool {
        self.modal.is_animating()
            || self.flash.as_ref().is_some_and(|(_, _, t)| t.animating())
            // The vectors are being made: frames until they're here (a moment).
            || self.making.is_some()
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

    pub fn has_query(&self) -> bool {
        !self.query.text().trim().is_empty()
    }

    pub fn type_text(&mut self, typed: &str) {
        if self.shortcuts.filter_focused() {
            self.shortcuts.type_text(typed);
            return;
        }
        if let Some(editing) = &mut self.editing {
            editing.input.insert(typed);
            return;
        }
        self.query_focused = true;
        self.query.insert(typed);
        self.selected = 0;
    }

    pub fn backspace(&mut self, word: bool) {
        if self.shortcuts.filter_focused() {
            self.shortcuts.backspace(word);
            return;
        }
        if let Some(editing) = &mut self.editing {
            editing.input.backspace(word);
            return;
        }
        self.query.backspace(word);
        self.selected = 0;
    }

    /// The field with the keys (for clipboard / select-all routing): a block's text field
    /// being written, else the search.
    pub fn query_input(&self) -> &ui_kit::TextInput {
        self.editing.as_ref().map_or(&self.query, |e| &e.input)
    }

    pub fn query_input_mut(&mut self) -> &mut ui_kit::TextInput {
        if let Some(editing) = &mut self.editing {
            return &mut editing.input;
        }
        self.query_focused = true;
        &mut self.query
    }

    /// A block's text field is being written.
    pub fn editing_text(&self) -> bool {
        self.editing.is_some()
    }

    /// Enter (or a click elsewhere): the text written, to save.
    pub fn commit_text(&mut self) -> Option<SettingsAction> {
        let Editing { block, key, input } = self.editing.take()?;
        Some(SettingsAction::SetText { block, key, value: input.text().trim().to_owned() })
    }

    /// Esc: the change given up (a field being written, a shortcut being recorded or its
    /// conflict, the shortcuts' search). `false` if there was none.
    pub fn cancel_text(&mut self) -> bool {
        self.editing.take().is_some() || self.shortcuts.cancel()
    }

    /// Settings → Shortcuts: `command` opened, recording its next keys.
    pub fn open_shortcut(&mut self, command: &str) {
        self.shortcuts.open_recording(command);
    }

    /// The next keys pressed are a shortcut being recorded (they don't act).
    pub fn recording_shortcut(&self) -> bool {
        self.shortcuts.recording()
    }

    /// Keys pressed while recording (as written, as shown): the change, unless others
    /// have them (then it asks).
    pub fn record_shortcut(&mut self, spec: &str, shown: &str) -> Option<SettingsAction> {
        self.shortcuts.record(spec, shown).map(SettingsAction::Shortcuts)
    }

    /// The shortcuts' search or a recording has the keys (as a field being written does).
    pub fn shortcuts_have_keys(&self) -> bool {
        self.shortcuts.filter_focused() || self.shortcuts.recording()
    }

    pub fn clear_query(&mut self) {
        self.query.set_text("");
        self.results.clear();
        self.searched = None;
    }

    /// ↑ / ↓ through the results.
    pub fn move_result(&mut self, delta: isize) -> bool {
        if self.results.is_empty() {
            return false;
        }
        self.selected = ui_kit::step_index(self.selected, delta, self.results.len());
        true
    }

    /// Enter: opens the selected result.
    pub fn confirm(&mut self, model: &SettingsModel) -> bool {
        let Some(&entry) = self.results.get(self.selected) else { return false };
        self.open_entry(model, entry);
        true
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

    /// The action at point `p` (a click outside the panel does nothing). Clicking
    /// a section activates it (no action for the app).
    pub fn hit(&mut self, p: Point) -> Option<SettingsAction> {
        // A dropdown's list open: it takes the click (a choice, or closed by one outside).
        if self.picker.is_open() {
            let event = self.picker.press(p);
            return self.picked(event);
        }
        // A dropdown's field (in the content): its list opens.
        if self.modal.hit(p) == Some(ModalHit::Content)
            && let Some(dropdown) = self.dropdowns.iter().find(|d| d.field.contains(p)).cloned()
        {
            let items = dropdown
                .choices
                .iter()
                .map(|(_, name)| ui_components::picker::PickerItem {
                    label: name.clone(),
                    ..Default::default()
                })
                .collect();
            let chosen = dropdown.choices.iter().position(|(v, _)| *v == dropdown.current);
            self.picker.open(items, chosen);
            self.picker_for = Some(dropdown);
            return None;
        }
        // The frame first: the content's rows scrolled under the header keep their rects
        // but are hidden there, so a click on esc closes instead of reaching a row.
        let at = self.modal.hit(p);
        if at == Some(ModalHit::Close) {
            return Some(SettingsAction::Close);
        }
        // The scrollbars: the thumb dragged or jumped to (nothing under them reached).
        if self.modal.press_nav_bar(p) {
            return None;
        }
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            self.limit_scroll();
            return None;
        }
        let in_content = at == Some(ModalHit::Content);
        // The shortcuts' editor: its search, its commands, their buttons.
        if in_content && self.shortcuts.over(p) {
            self.query_focused = false;
            return self.shortcuts.click(p).map(SettingsAction::Shortcuts);
        }
        self.shortcuts.blur();
        // A block's text field: a click on it starts writing it; while one is written, a
        // click elsewhere saves it (and does nothing else).
        let field = self.text_fields.iter().find(|(r, ..)| in_content && r.contains(p)).cloned();
        if let Some(editing) = &self.editing {
            if field.as_ref().is_some_and(|(_, b, k, _)| *b == editing.block && *k == editing.key) {
                return None;
            }
            return self.commit_text();
        }
        if let Some((_, block, key, value)) = field {
            // A secret is written from empty, as dots (what's stored is never shown).
            let secret = self.secret_fields.contains(&(block, key));
            let mut input = ui_kit::TextInput::new(if secret { "" } else { &value });
            input.set_masked(secret);
            input.select_all();
            self.editing = Some(Editing { block, key, input });
            self.query_focused = false;
            return None;
        }
        self.query_focused = self.field.contains(p);
        if let Some(i) = self.filter_rects.iter().position(|r| in_content && r.contains(p)) {
            self.theme_filter = ThemeFilter::ALL[i];
            return None;
        }
        if self.clear.is_some_and(|r| r.contains(p)) {
            self.clear_query();
            return None;
        }
        if let Some(i) = self.result_rects.iter().position(|r| in_content && r.contains(p)) {
            self.selected = i;
            return Some(SettingsAction::OpenResult);
        }
        match at? {
            ModalHit::Close => Some(SettingsAction::Close),
            // Its footer has no buttons, nor a zoom.
            ModalHit::Button(_) | ModalHit::Zoom(_) => None,
            ModalHit::Section(id) => {
                self.section = Some(id);
                self.scroll = 0.0;
                None
            }
            ModalHit::Content => {
                let action = self.content_hit(p);
                if let Some(SettingsAction::ToggleAdvanced(page)) = action {
                    self.toggle_advanced(page);
                    return None;
                }
                // On a slider: the following moves drag it.
                self.slider_held = match &action {
                    Some(SettingsAction::Choose { value, .. }) => self
                        .sliders
                        .iter()
                        .position(|(a, ..)| a.rect.contains(p))
                        .map(|i| (i, *value)),
                    _ => None,
                };
                action
            }
        }
    }

    fn content_hit(&self, p: Point) -> Option<SettingsAction> {
        if self.studio.contains(p) {
            return Some(SettingsAction::OpenThemeStudio);
        }
        if self.toggle.contains(p) {
            return Some(SettingsAction::ToggleFollowSystem);
        }
        if let Some((_, set)) = self.file_icon_cards.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectFileIcons(*set));
        }
        if let Some((_, style)) = self.icon_cards.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectIcons(*style));
        }
        if let Some((_, id)) = self.framework_rows.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::ToggleFramework(id.clone()));
        }
        if let Some((area, block, key, values)) =
            self.sliders.iter().find(|(a, ..)| a.rect.contains(p))
        {
            let value = *values.get(area.nearest(p.x)?)?;
            return Some(SettingsAction::Choose { block, key, value });
        }
        if let Some((_, action)) = self.block_hits.iter().find(|(r, _)| r.contains(p)) {
            return Some(action.clone());
        }

        if let Some((_, block, key, value)) = self.text_fields.iter().find(|(r, ..)| r.contains(p))
        {
            return Some(SettingsAction::SetText { block, key, value: value.clone() });
        }
        if let Some((_, action)) = self.module_hits.iter().find(|(r, _)| r.contains(p)) {
            return Some(action.clone());
        }
        if let Some((_, code)) = self.languages.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectLanguage(code.clone()));
        }
        if let Some((_, id)) = self.keymap_chips.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectKeymap(id.clone()));
        }
        self.cards
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, id)| SettingsAction::SelectTheme(id.clone()))
    }

    pub fn hover(&mut self, p: Point) -> bool {
        self.mouse = Some(p);
        if self.picker.is_open() {
            return self.picker.hover(p);
        }
        let bar = self.bar.hover(p);
        // Rows scrolled under the header aren't lit there.
        let in_content = self.modal.hit(p) == Some(ModalHit::Content);
        let shortcut = if in_content { p } else { Point::new(f32::MIN, f32::MIN) };
        let frame = self.modal.hover(p) | self.shortcuts.hover(shortcut);
        let hover = match self.modal.hit(p) {
            Some(ModalHit::Content) => self.content_hit(p),
            _ => None,
        };
        let clear = self.clear.is_some_and(|r| r.contains(p));
        let filter = self.filter_rects.iter().position(|r| r.contains(p));
        let changed =
            hover != self.hover || clear != self.clear_hover || filter != self.filter_hover;
        self.filter_hover = filter;
        self.hover = hover;
        self.clear_hover = clear;
        frame || changed || bar
    }

    /// A slider is held (pressed): the mouse's moves drag it.
    pub fn slider_held(&self) -> bool {
        self.slider_held.is_some()
    }

    /// The held slider dragged to `p`: the value of the stop nearest to it, when it changes.
    pub fn drag(&mut self, p: Point) -> Option<SettingsAction> {
        let (i, last) = self.slider_held?;
        let (area, block, key, values) = self.sliders.get(i)?;
        let value = *values.get(area.nearest(p.x)?)?;
        if value == last {
            return None;
        }
        self.slider_held = Some((i, value));
        Some(SettingsAction::Choose { block, key, value })
    }

    /// The mouse was released (a held slider lets go). `true` if one was held.
    pub fn release(&mut self) -> bool {
        let bars = self.bar.release() | self.modal.release_nav_bar();
        self.slider_held.take().is_some() | bars
    }

    /// Something clickable is under the mouse (the cursor becomes a hand).
    pub fn over_clickable(&self, p: Point) -> bool {
        let in_content = self.modal.hit(p) == Some(ModalHit::Content);
        matches!(self.modal.hit(p), Some(ModalHit::Section(_)))
            || (in_content
                && (self.content_hit(p).is_some()
                    || self.shortcuts.over(p)
                    || self.result_rects.iter().any(|r| r.contains(p))
                    || self.filter_rects.iter().any(|r| r.contains(p))))
            || self.clear.is_some_and(|r| r.contains(p))
    }

    /// Opens the result chosen with the mouse (the `OpenResult` action).
    pub fn open_selected(&mut self, model: &SettingsModel) {
        self.confirm(model);
    }

    pub fn paint(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.forget_painted();
        // The entries are only needed while searching.
        let entries = if self.has_query() { self.entries_of(model) } else { Default::default() };
        self.search(&entries);
        let pages = pages(&model.blocks);
        let groups = group_labels();
        let sections = modal_sections(&pages, &groups);
        self.section_ids = sections.iter().map(|s| s.id).collect();
        let title = i18n::t("settings.title");
        let subtitle = i18n::t("settings.subtitle");
        let hint_texts = self.key_hints();
        let hints: Vec<KeyHint<'_>> =
            hint_texts.iter().map(|(keys, what)| (*keys, what.as_str())).collect();
        let spec = ModalSpec {
            title: &title,
            subtitle: Some(&subtitle),
            icon: Some("gear"),
            size: ModalSize::XLarge,
            sections: &sections,
            active: if self.has_query() { None } else { Some(self.active()) },
            // The keys that work now, as the list modals show theirs (the footer stays at the
            // bottom; the navigation and the content scroll above it).
            hints: &hints,
            buttons: &[],
            status: None,
            nav_header: 0.0,
            header_field: SEARCH_FIELD_W,
            fit: None,
            fit_width: None,
            title_widens: true,
            closable: true,
            zoom: None,
        };
        let Some((content, ctx)) = self.modal.paint(&spec, icons, text, scene, palette) else {
            return;
        };
        self.paint_search_field(icons, text, scene, &ctx);
        let layer = scene.push_layer(Some(content));
        let pad = ctx.px(28.0);
        let x0 = content.min_x() + pad;
        let width = content.size.width - 2.0 * pad;
        self.content = content;
        // Whole pixels: a fractional scroll (trackpad momentum) would move text and boxes
        // out of step by a pixel (jitter).
        let y = content.min_y() + ctx.px(24.0) - self.scroll.round();
        if self.has_query() {
            self.paint_results(
                &entries,
                &sections,
                icons,
                SectionArea { layer, text, ctx: &ctx, x0, top: y, width },
            );
            self.clamp_scroll(ctx.px(24.0));
            // No cards among the results: their previews let go.
            self.paint_card_pictures(scene, text, palette, ctx.scale);
            self.paint_bar(scene, palette, ctx.px(1.0));
            return;
        }
        let active = self.active();
        let area = SectionArea { layer, text, ctx: &ctx, x0, top: y, width };
        // The app's pages draw their own content first; a page's blocks follow it.
        let fixed = match active {
            MODULES => {
                self.paint_modules(model, area);
                true
            }
            ICONS => {
                self.paint_icon_sets(model, icons, area);
                true
            }
            KEYMAP => {
                let label = i18n::t("settings.keymap");
                self.paint_keymap(model, icons, area, &label);
                true
            }
            LANGUAGE => {
                let label = i18n::t("settings.language");
                self.paint_languages(model, area, &label);
                true
            }
            APPEARANCE => {
                self.paint_appearance(model, area);
                true
            }
            _ => false,
        };
        if !fixed || model.blocks.iter().any(|b| b.page == active) {
            let top = if fixed { self.painted_bottom() + ctx.px(24.0) } else { y };
            self.paint_blocks(
                model,
                icons,
                active,
                SectionArea { layer, text, ctx: &ctx, x0, top, width },
            );
        }
        self.clamp_scroll(ctx.px(24.0));
        self.paint_revealed(layer, &ctx);
        // The cards' pictures, over the content's layer (the modules push their own).
        self.paint_card_pictures(scene, text, palette, ctx.scale);
        self.paint_bar(scene, palette, ctx.px(1.0));
        // A dropdown's list, over everything, from its field (where it is now).
        if let Some(open) = &self.picker_for
            && self.picker.is_open()
        {
            let field = self
                .dropdowns
                .iter()
                .find(|d| d.block == open.block && d.key == open.key)
                .map_or(open.field, |d| d.field);
            self.picker.paint(scene, text, icons, &ctx, (field, self.viewport), "");
        }
    }

    /// A dropdown's list is open (it has the keys: ↑ ↓ ↵, Esc closes it).
    pub fn picker_open(&self) -> bool {
        self.picker.is_open()
    }

    /// A key for an open dropdown's list: what it chose, when it did.
    pub fn picker_key(&mut self, key: ui_components::ViewerKey) -> Option<SettingsAction> {
        let event = self.picker.key(key, ui_components::KeyReach::default());
        self.picked(event)
    }

    /// What the list's `event` means: a choice of its row's (saved like a pill's).
    fn picked(
        &mut self,
        event: Option<ui_components::picker::PickerEvent>,
    ) -> Option<SettingsAction> {
        use ui_components::picker::PickerEvent;
        match event {
            Some(PickerEvent::Chosen(i)) => {
                let dropdown = self.picker_for.take()?;
                let (value, _) = dropdown.choices.get(i)?;
                Some(SettingsAction::Choose {
                    block: dropdown.block,
                    key: dropdown.key,
                    value: *value,
                })
            }
            Some(PickerEvent::Closed) => {
                self.picker_for = None;
                None
            }
            _ => None,
        }
    }

    /// The content's scrollbar along its right edge, in its margin (nothing when it fits).
    fn paint_bar(&mut self, scene: &mut Scene, palette: &Palette, scale: f32) {
        let area = self.content;
        self.bar.paint(scene, palette, scale, area, self.content_height, self.scroll.round());
    }

    /// The keys that work in the settings now, for its footer: while a field is written,
    /// ↵ saves and Esc gives it up; while searching, ↑ ↓ walk the results, ↵ opens one and
    /// Esc clears the search; else ↑ ↓ change the page, typing searches and Esc closes.
    pub fn key_hints(&self) -> Vec<(&'static [&'static str], String)> {
        let t = i18n::t;
        if self.editing_text() || self.shortcuts_have_keys() {
            return vec![(&["↵"], t("settings.hint_save")), (&["esc"], t("settings.hint_cancel"))];
        }
        if self.has_query() {
            return vec![
                (&["↑", "↓"], t("settings.hint_results")),
                (&["↵"], t("settings.hint_open")),
                (&["esc"], t("settings.hint_clear")),
            ];
        }
        vec![
            (&["↑", "↓"], t("settings.hint_pages")),
            (&["A–Z"], t("settings.hint_type")),
            (&["esc"], t("keys.close")),
        ]
    }

    /// One of the scrollbars (the navigation's, the content's) is held: the mouse's moves
    /// drag it.
    pub fn scrollbar_held(&self) -> bool {
        self.bar.is_dragging() || self.modal.nav_bar_held()
    }

    /// The held scrollbar dragged to `p`: `true` if it moved.
    pub fn drag_scrollbar(&mut self, p: Point) -> bool {
        if self.modal.nav_bar_held() {
            return self.modal.drag_nav_bar(p);
        }
        match self.bar.drag(p) {
            Some(offset) => {
                let before = self.scroll;
                self.scroll = offset;
                self.limit_scroll();
                before != self.scroll
            }
            None => false,
        }
    }

    /// Forgets what the last paint recorded (the clickable areas, the anchors), before
    /// painting again.
    fn forget_painted(&mut self) {
        self.shortcuts.forget();
        self.cards.clear();
        self.filter_rects.clear();
        self.languages.clear();
        self.keymap_chips.clear();
        self.binding_rows.clear();
        self.toggle = Rect::default();
        self.icon_cards.clear();
        self.file_icon_cards.clear();
        self.framework_rows.clear();
        self.module_rows.clear();
        self.module_hits.clear();
        self.block_hits.clear();
        self.dropdowns.clear();
        self.sliders.clear();
        self.text_fields.clear();
        self.secret_fields.clear();
        // Nothing painted yet (not 0: a page scrolled past its top paints above 0, and a 0
        // counted as its bottom put the blocks at a fixed place and made it grow each frame).
        self.blocks_bottom = f32::NEG_INFINITY;
        self.row_anchors.clear();
        self.studio = Rect::default();
        self.result_rects.clear();
        self.anchors.clear();
        self.pictures.pending.clear();
    }

    /// A search result was opened: its element is brought into view (next frame) and
    /// highlighted.
    fn paint_revealed(&mut self, layer: &mut Layer, ctx: &Ctx<'_>) {
        let flashed = self.flash_rect();
        if std::mem::take(&mut self.reveal)
            && let Some(r) = flashed
        {
            let view = self.content;
            let margin = ctx.px(24.0);
            if r.min_y() < view.min_y() + margin || r.max_y() > view.max_y() - margin {
                self.scroll += r.min_y() - (view.min_y() + view.size.height / 3.0);
                self.limit_scroll();
            }
        }
        let value = self.flash.as_ref().map_or(0.0, |(_, _, t)| t.value());
        Self::paint_flash(layer, ctx, flashed, value);
    }

    /// Measures the content just painted (its height doesn't depend on the scroll) and
    /// keeps the scroll within bounds. Only rects from this paint count: stale ones from
    /// another section, or positions from a previous scroll, made the content look taller
    /// (scrolling past the end) and the limit jump between frames (jitter with momentum).
    fn clamp_scroll(&mut self, margin: f32) {
        let top = self.content.min_y() - self.scroll.round();
        self.content_height = self.painted_bottom().max(top) - top + margin;
        if std::mem::take(&mut self.scroll_to_end) {
            self.scroll = f32::MAX;
        }
        self.limit_scroll();
    }

    /// Where what was painted last ends (its lowest element).
    fn painted_bottom(&self) -> f32 {
        let top = self.content.min_y() - self.scroll.round();
        self.cards
            .iter()
            .map(|(r, _)| r.max_y())
            .chain(self.languages.iter().map(|(r, _)| r.max_y()))
            .chain(self.keymap_chips.iter().map(|(r, _)| r.max_y()))
            .chain(self.binding_rows.iter().map(Rect::max_y))
            .chain(self.framework_rows.iter().map(|(r, _)| r.max_y()))
            .chain(self.module_rows.iter().map(Rect::max_y))
            .chain(self.module_hits.iter().map(|(r, _)| r.max_y()))
            .chain(self.result_rects.iter().map(Rect::max_y))
            .chain(
                [self.toggle, self.studio]
                    .iter()
                    .filter(|r| **r != Rect::default())
                    .map(Rect::max_y),
            )
            .chain([self.blocks_bottom])
            .chain(self.icon_cards.iter().map(|(r, _)| r.max_y()))
            .chain(self.file_icon_cards.iter().map(|(r, _)| r.max_y()))
            .fold(top, f32::max)
    }

    fn limit_scroll(&mut self) {
        let max = (self.content_height - self.content.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    /// The mouse wheel: over the navigation it scrolls the navigation (when it's taller than
    /// its room; at its end nothing moves), anywhere else the content. `true` if it scrolled.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        // A dropdown's list open: the wheel scrolls it (nothing behind it moves).
        if self.picker.is_open() {
            return self.picker.scroll(dy);
        }
        if let Some(p) = self.mouse
            && self.modal.over_nav(p)
        {
            return self.modal.scroll_nav(p, dy);
        }
        let before = self.scroll;
        self.scroll += dy;
        self.limit_scroll();
        self.scroll != before
    }
}

#[cfg(test)]
#[path = "../../tests/unit/settings_view.rs"]
mod tests;
