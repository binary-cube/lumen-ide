//! "Theme configurator" module: a tab where you build your theme starting from an existing one.
//!
//! Three areas: the categories down the left (General, Interface, Syntax, Caret, Effects and
//! the theme's own settings, each with a dot while it has unsaved changes), the chosen
//! category in the middle (its settings in groups, a row each: its label, a line saying what
//! it does, its control, "↺" while it differs from the theme it started from), and a live
//! preview on the right. A search above finds any setting across the categories and jumps to
//! it; the number of unsaved changes is shown beside it, and "Discard" undoes them all.
//! Narrow, the categories become tabs and the preview hides behind a button. Every change is
//! also visible in the editor right away (a temporary theme, `PREVIEW_ID`). The saved theme
//! goes into the global themes folder and is used only in the current project or
//! everywhere; its decorations may also be used with every theme.
//!
//! The module does not touch themes directly: it sends the app requests (`theme.preview`,
//! `theme.save.project`, `theme.save.global`, `theme.cancel`) with the theme text.

use std::path::Path;
use std::sync::Arc;

use code_intelligence::{ColorFormat, ColorLiteral, ColorProvider};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::t;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use theming::caret::{Blink, CaretLook, CaretShape, CaretSpec, Motion};
use theming::decorations::{DecorationLayer, LayerSpec};
use theming::studio::{PREVIEW_ID, StudioInput, ThemeDraft, compose};
use ui_components::color_picker::ColorPart;
use ui_components::control::{Control, Controls, Shown, Thumb};
use ui_components::ds::{self, Ctx, radius};
use ui_components::search_field::SearchField;
use ui_components::{
    AppRequest, ColorPicker, LayerPainter, PickerAction, ViewMenuEntry, Viewer, ViewerAction,
    ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::{TextInput, TextInputStyle, draw_text};

mod effects;
mod layout;
mod model;
mod panel;
mod parts;
mod preview;
mod rows;
mod view;
use model::{Category, SettingId};
use preview::*;
use rows::Row;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("view.theme-studio"),
    name: "Theme studio",
    description: "Your own themes: interface and syntax colors, the caret, effects, seen live",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "palette", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct ThemeStudioModule;

impl Module for ThemeStudioModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: THEME_STUDIO,
                label: "Theme Studio…",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::APPEARANCE_THEME,
                    20,
                )),
                shortcuts: &[],
            },
            icon: Some("droplet"),
            handler: Arc::new(OpenStudio),
        });
        Ok(())
    }
}

/// Opens the configurator (the workbench's command, implemented here).
pub const THEME_STUDIO: command_model::CommandId = workbench_model::THEME_STUDIO;

/// The configurator in a tab, with the host's themes (one open keeps the user's draft).
struct OpenStudio;

impl workbench_model::CommandHandler for OpenStudio {
    fn run(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        let Some(input) = host.themes() else { return false };
        host.open_view(&t("studio.title"), EXTENSION, &input)
    }
}

/// Extension of the configurator's virtual path (the app closes it once a theme is saved).
const EXTENSION: &str = theming::studio::STUDIO_VIEW;

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        "theme-studio"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(Studio::default()))
    }
}

/// The color picker works with hex (themes are written in hex).
#[derive(Debug)]
struct Hex;

impl ColorProvider for Hex {
    fn find(&self, _language: &str, _line: &str) -> Vec<ColorLiteral> {
        Vec::new()
    }

    fn parse(&self, text: &str) -> Option<ColorLiteral> {
        let text = text.trim();
        let color = Rgba::from_hex(text)?;
        Some(ColorLiteral { range: 0..text.len(), color, format: ColorFormat::Hex })
    }

    fn format(&self, color: Rgba, _format: ColorFormat) -> String {
        let [r, g, b, a] = color.to_rgba8();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// Which themes the gallery of starting points shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum BaseFilter {
    #[default]
    All,
    Dark,
    Light,
}

impl BaseFilter {
    const ALL: [Self; 3] = [Self::All, Self::Dark, Self::Light];

    fn label(self) -> String {
        t(match self {
            Self::All => "studio.filter_all",
            Self::Dark => "studio.filter_dark",
            Self::Light => "studio.filter_light",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Hit {
    /// A category chosen (the navigation).
    Category(Category),
    /// A theme to start from (its place in the input).
    Base(usize),
    BaseFilter(BaseFilter),
    /// A row's choice (the caret's, a layer's, the theme's settings', the presets…): the
    /// row, the choice.
    Choice(Row, usize),
    /// A color to pick (its key in the draft).
    Token(String),
    /// A color put back as the starting theme has it.
    ResetToken(String),
    /// A row put back as the starting theme has it.
    ResetRow(Row),
    /// One color more / fewer in a layer's list: the layer, its option.
    AddColor(usize, &'static str),
    DropColor(usize, &'static str),
    /// The decorations used with every theme.
    Overlay,
    Name,
    /// The search field (its × too: the field's own); a found setting (its place in the
    /// results).
    Search,
    Found(usize),
    /// An effect's card: opened / closed (its header), dragged (its handle), switched on /
    /// off, its menu (the app's, under its button); one of its sections opened / closed.
    Fold(usize),
    Handle(usize),
    Toggle(usize),
    Menu(usize),
    Section(usize, effects::Sec),
    /// The gallery of effects opened (`true`) or closed.
    Gallery(bool),
    /// One of the decorations' own settings chosen (its value).
    Global(model::Global, i64),
    /// The preview: moving or still; dark or light (a theme with both); shown (narrow).
    PreviewMoving(bool),
    PreviewLight(bool),
    PreviewOpen,
    Scope(bool),
    Save,
    /// Undoes every change (or, with none, closes the studio).
    Discard,
}

/// The places of the studio's parts among its controls (see [`Studio::with_controls`]).
const PICKER: usize = 0;
const SLIDERS: usize = 2;
const NAME: usize = 4;
const QUERY: usize = 5;
const HANDLE: usize = 6;

#[derive(Debug, Default)]
struct Studio {
    input: Option<StudioInput>,
    /// The theme it started from (index into `input.themes`).
    base: usize,
    /// The starting theme as read (what "↺" and the changes compare with).
    base_draft: Option<ThemeDraft>,
    /// Every theme's colors, read once (the gallery's thumbnails, the dark / light filter).
    thumbs: Vec<Option<ThemeDraft>>,
    draft: Option<ThemeDraft>,
    name: TextInput,
    /// The search across every setting (the app's search field: ×, ⌘A; its focus the
    /// controls').
    query: SearchField,
    /// What a key in the name's or the search's field asked (see [`parts::FieldPart`]).
    name_asked: Option<parts::FieldAsk>,
    query_asked: Option<parts::FieldAsk>,
    category: Category,
    /// Each category's scroll (in the order of [`Category::ALL`]).
    scrolls: [f32; 6],
    /// The settings' scrollbar, hidden at rest (over the panel's right padding).
    bar: ui_components::EdgeScrollbar,
    /// Its controls' state of use (see [`Studio::with_controls`]).
    controls: Controls,
    base_filter: BaseFilter,
    /// Saved globally (otherwise only for the current project).
    global: bool,
    picker: ColorPicker,
    /// The color being edited and its previous value (for "Cancel").
    picking: Option<(String, Rgba)>,
    /// What the picker asked (applied, given up, the eyedropper), and its color changed by a
    /// key or a text: the draft follows.
    picker_asked: Option<PickerAction>,
    picker_recolored: bool,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    /// The sliders, where they were drawn, and the one held; the one the keys step.
    sliders: Vec<(ui_components::SliderArea, Row)>,
    slider_held: Option<Row>,
    slider_focused: Option<Row>,
    /// What a slider asked (a stop chosen, a step).
    slider_asked: Option<parts::SliderAsk>,
    /// Where each setting's row was drawn last (a jump from the search goes there).
    rows_at: Vec<(SettingId, Rect)>,
    /// The setting jumped to from the search: scrolled to once drawn, then highlighted.
    jump: Option<SettingId>,
    highlight: Option<SettingId>,
    /// Where the name's field was drawn (its text goes over it).
    name_field: Option<Rect>,
    /// Each section's header as drawn last (its id, where, the space left above it):
    /// `scroll-to:<id>` goes there.
    headers: Vec<(&'static str, Rect, f32)>,
    /// The decorations were just handed over to be used with every theme.
    overlay_saved: bool,
    /// The preview: moving (else a still frame), in the light variant (a theme with both),
    /// shown over the panel (narrow).
    preview_still: bool,
    preview_light: bool,
    preview_open: bool,
    /// The preview shows a dialog on the draft's backdrop (while that's being changed).
    backdrop_shown: bool,
    /// The effects' cards: which are open, their sections opened or closed by hand (the
    /// effect, the section, open), where each was drawn, the one dragged; an effect's menu
    /// asked for under its button, for the app to show.
    open_layers: Vec<bool>,
    sections: Vec<(usize, effects::Sec, bool)>,
    cards_at: Vec<(usize, Rect)>,
    drag: Option<effects::Drag>,
    asked_menu: Option<(Rect, Vec<ViewMenuEntry>)>,
    /// The gallery of effects: open, scrolled, how tall its content is, its thumbnails'
    /// painters (kept while it's open), one moving under the mouse.
    gallery_open: bool,
    gallery_scroll: f32,
    gallery_h: f32,
    /// Scroll the gallery to its end once its height is known (`gallery:end`, snapshots).
    gallery_to_end: bool,
    gallery_painters: Vec<(String, Box<dyn ui_components::decorations::WindowDecorations>)>,
    gallery_moving: bool,
    /// The preview's decorations, and when they started moving; the last input (they stop
    /// when idle).
    live: Option<effects::Live>,
    anim_start: Option<std::time::Instant>,
    last_input: Option<std::time::Instant>,
    bounds: Rect,
    scale: f32,
    /// The middle panel (what scrolls).
    list: Rect,
    content_h: f32,
    pending: Option<ViewerAction>,
}

impl Studio {
    /// The id a saved theme named `name` gets: the edited theme's own when it's one made
    /// here and keeps its name (saving it again), else a new one (UUID v4: two themes
    /// named alike never overwrite each other).
    fn saved_id(&self, name: &str) -> String {
        let base = self.input.as_ref().and_then(|i| i.themes.get(self.base));
        match base {
            Some(theme) if theme.id.starts_with("custom-") && theme.name == name => {
                theme.id.clone()
            }
            _ => format!("custom-{}", uuid::Uuid::new_v4()),
        }
    }

    fn template(&self) -> Option<&str> {
        self.input.as_ref()?.themes.get(self.base).map(|theme| theme.source.as_str())
    }

    fn load_base(&mut self, i: usize) {
        self.base = i;
        let draft = self.input.as_ref().and_then(|input| input.themes.get(i)?.draft());
        self.base_draft = draft.clone();
        self.draft = draft;
        self.overlay_saved = false;
        self.preview_light = false;
        if self.draft.as_ref().is_some_and(|d| d.options.is_empty())
            && self.category == Category::Options
        {
            self.category = Category::General;
        }
        self.preview();
    }

    /// The work-in-progress theme, applied live in the editor (temporary).
    fn preview(&mut self) {
        let (Some(template), Some(draft)) = (self.template(), &self.draft) else { return };
        let mut draft = draft.clone();
        draft.id = PREVIEW_ID.to_owned();
        draft.name = self.name.text().to_owned();
        if let Some(src) = compose(template, &draft) {
            self.pending = Some(ViewerAction::Request(AppRequest::PreviewTheme(src)));
        }
    }

    fn save(&mut self) {
        let (Some(template), Some(draft)) = (self.template(), &self.draft) else { return };
        let mut draft = draft.clone();
        let name = self.name.text().trim();
        draft.name = if name.is_empty() { t("studio.default_name") } else { name.to_owned() };
        draft.id = self.saved_id(&draft.name);
        if let Some(src) = compose(template, &draft) {
            let request = AppRequest::SaveTheme { source: src, global: self.global };
            self.pending = Some(ViewerAction::Request(request));
        }
    }

    /// Every setting it shows now.
    fn entries(&self) -> Vec<model::Entry> {
        match (&self.draft, &self.input) {
            (Some(d), Some(i)) => model::catalog(d, i, false),
            _ => Vec::new(),
        }
    }

    /// How many unsaved changes the draft has (the tests').
    #[cfg(test)]
    fn changes(&self) -> usize {
        match (&self.draft, &self.base_draft) {
            (Some(d), Some(b)) => model::changes(&self.entries(), d, b),
            _ => 0,
        }
    }

    /// Every change undone: the draft as the starting theme has it (the name kept).
    fn discard(&mut self) {
        self.draft = self.base_draft.clone();
        self.close_picker();
        self.picking = None;
        self.preview();
    }

    /// Color `key` as the starting theme has it.
    fn reset_token(&mut self, key: &str) {
        let Some(color) = self.base_draft.as_ref().and_then(|b| b.get(key)) else { return };
        if let Some(draft) = &mut self.draft {
            draft.set(key, color);
        }
        self.preview();
    }

    /// Row `row` as the starting theme has it (a layer's option: as its layer there has it
    /// when it's of the same kind, else its kind's default).
    fn reset_row(&mut self, row: Row) {
        let (Some(draft), Some(base)) = (&mut self.draft, &self.base_draft) else { return };
        rows::reset(row, draft, base);
        self.preview();
    }

    /// The draft's decorations handed to the decorations module, to be drawn with every
    /// theme (as its settings keep them: one layer a line).
    fn use_everywhere(&mut self) {
        let Some(draft) = &self.draft else { return };
        let value: Vec<String> = draft
            .decorations
            .layers
            .iter()
            .filter(|l| l.enabled)
            .map(|l| theming::decorations::encode(&l.spec))
            .collect();
        self.pending = Some(ViewerAction::Ask(ui_components::Asked::new(
            contracts::decorations::SetOwnDecorations { layers: value.join("\n") },
        )));
        self.overlay_saved = true;
    }

    /// The picker's color, applied to the work-in-progress theme (while dragging).
    fn sync_picker_color(&mut self) {
        if let (Some((key, _)), Some(draft)) = (&self.picking, &mut self.draft) {
            self.backdrop_shown = key == theming::studio::BACKDROP_COLOR;
            draft.set(key, self.picker.color());
            self.preview();
        }
    }

    fn picker_action(&mut self, action: PickerAction) {
        // The eyedropper: the app picks from the screen and sends the color back as the
        // `screen-color:<hex>` command; the picker stays open meanwhile.
        if action == PickerAction::Eyedropper {
            self.pending = Some(ViewerAction::Request(AppRequest::SampleScreen));
            return;
        }
        let Some((key, before)) = self.picking.take() else { return };
        if let Some(draft) = &mut self.draft {
            match action {
                PickerAction::Apply(color, _) => draft.set(&key, color),
                PickerAction::Cancel => draft.set(&key, before),
                PickerAction::Eyedropper => {}
            }
        }
        self.close_picker();
        self.preview();
    }

    /// The picker opened at `swatch` on `color`: it takes the keys while it's open (they're
    /// the focused part's again once it closes: the focus stays under it).
    fn open_picker(&mut self, swatch: Rect, color: Rgba) {
        self.picker.open(swatch, color, ColorFormat::Hex, &Hex);
    }

    /// The picker closed: the keys are the focused part's again.
    fn close_picker(&mut self) {
        self.picker.close();
    }

    /// The part with the keys (under the picker while it's open).
    fn focused_part(&self) -> Option<usize> {
        self.controls.focused()
    }

    /// The keys given to part `to` (`None`: to none); while the picker is open, once it
    /// closes.
    fn focus_part(&mut self, to: Option<usize>) {
        self.with_controls(|c, parts| c.focus(parts, to));
    }

    /// Part `part` lets go of the keys (if it has them).
    fn leave(&mut self, part: usize) {
        if self.focused_part() == Some(part) {
            self.focus_part(None);
        }
    }

    /// The name's field has the keys.
    fn editing_name(&self) -> bool {
        self.focused_part() == Some(NAME)
    }

    /// The search's field has the keys.
    fn editing_query(&self) -> bool {
        self.focused_part() == Some(QUERY)
    }

    /// A slider at `x`: its nearest stop chosen (shown at once). `true` if it changed.
    fn slide(&mut self, area: &ui_components::SliderArea, row: Row, x: f32) -> bool {
        let Some(i) = area.nearest(x) else { return false };
        self.set_stop(row, i)
    }

    /// Row `row` at stop `i` (a slider's); `true` if it changed.
    fn set_stop(&mut self, row: Row, i: usize) -> bool {
        let Some(draft) = &mut self.draft else { return false };
        if row.current(draft) == i && row.said(draft) {
            return false;
        }
        row.choose(draft, i);
        self.backdrop_shown = matches!(row, Row::Backdrop(_) | Row::Floating(_));
        self.preview();
        true
    }

    fn scroll_mut(&mut self) -> &mut f32 {
        let at = Category::ALL.iter().position(|c| *c == self.category).unwrap_or(0);
        &mut self.scrolls[at]
    }

    /// Its controls, the topmost first: the color picker (over everything while it's open),
    /// the settings' scrollbar with the shown category's scroll (not under the gallery), the
    /// sliders, what an open menu or the gallery lets through, the name's and the search's
    /// fields, the effects' handles. Under the open picker, the others aren't shown but the
    /// fields (one may keep the focus under it: the picker over them takes every press).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let at = Category::ALL.iter().position(|c| *c == self.category).unwrap_or(0);
        let picking = self.picker.is_open();
        let mut color = ColorPart {
            picker: &mut self.picker,
            colors: &Hex,
            action: &mut self.picker_asked,
            recolored: &mut self.picker_recolored,
        };
        let mut settings = Thumb { bar: &mut self.bar, offset: &mut self.scrolls[at] };
        let mut sliders = parts::SliderPart {
            sliders: &self.sliders,
            list: self.list,
            held: &mut self.slider_held,
            focused: &mut self.slider_focused,
            asked: &mut self.slider_asked,
        };
        let mut catch = parts::Catch { hits: &self.hits, gallery_open: self.gallery_open };
        let mut name = parts::FieldPart {
            input: &mut self.name,
            own: Hit::Name,
            hits: &self.hits,
            asked: &mut self.name_asked,
        };
        let mut query = parts::SearchPart {
            field: &mut self.query,
            hits: &self.hits,
            asked: &mut self.query_asked,
        };
        let mut handle = parts::HandlePart { hits: &self.hits, drag: &mut self.drag };
        let mut parts: [&mut dyn Control; 7] = [
            &mut Shown { part: &mut color, shown: picking },
            &mut Shown { part: &mut settings, shown: !self.gallery_open },
            &mut Shown { part: &mut sliders, shown: !picking },
            &mut Shown { part: &mut catch, shown: !picking },
            &mut name,
            &mut query,
            &mut Shown { part: &mut handle, shown: !picking },
        ];
        f(&mut self.controls, &mut parts)
    }

    fn scrolled(&self) -> f32 {
        let at = Category::ALL.iter().position(|c| *c == self.category).unwrap_or(0);
        self.scrolls[at]
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_h - self.list.size.height).max(0.0);
        let s = self.scroll_mut();
        *s = s.clamp(0.0, max);
    }

    /// Category `c` shown (a theme without settings of its own has no such category).
    fn show(&mut self, c: Category) -> bool {
        if c == Category::Options && self.draft.as_ref().is_none_or(|d| d.options.is_empty()) {
            return false;
        }
        self.category = c;
        self.leave(SLIDERS);
        true
    }

    /// The category holding `id` (`scroll-to:` / `category:`: a category's id, or a
    /// section's as the first studio named them — `caret`, `decorations`, `options`).
    fn scroll_to(&mut self, id: &str) -> bool {
        let category = match id {
            "decorations" => Some(Category::Effects),
            "name" | "start" => Some(Category::General),
            other => Category::parse(other),
        };
        if let Some(c) = category {
            let shown = self.show(c);
            if shown {
                *self.scroll_mut() = 0.0;
            }
            return shown;
        }
        let Some((_, r, _)) = self.headers.iter().find(|(h, ..)| *h == id) else { return false };
        let to = r.min_y() - self.list.min_y() - 16.0 * self.scale;
        *self.scroll_mut() += to;
        self.clamp_scroll();
        true
    }

    /// A found setting (`n`th of the search's results) shown: its category, scrolled to it
    /// once drawn, highlighted.
    fn jump_to(&mut self, n: usize) {
        let query = self.query.text().to_owned();
        let entries = self.entries();
        let Some(entry) = model::search(&entries, &query).get(n).map(|e| (*e).clone()) else {
            return;
        };
        self.query.set_text("");
        self.leave(QUERY);
        self.show(entry.category);
        self.jump = Some(entry.id.clone());
        self.highlight = Some(entry.id);
    }

    /// After a frame: a jump waiting for its row scrolls there.
    fn finish_jump(&mut self) -> bool {
        let Some(id) = self.jump.take() else { return false };
        let Some((_, r)) = self.rows_at.iter().find(|(i, _)| *i == id).cloned() else {
            return false;
        };
        let to = r.min_y() - self.list.min_y() - 48.0 * self.scale;
        *self.scroll_mut() += to;
        self.clamp_scroll();
        true
    }

    /// A hit in the scrolled panel, only its visible part (one scrolled under the header or
    /// the buttons must not take their clicks).
    fn push_list_hit(&mut self, r: Rect, hit: Hit) {
        if let Some(r) = r.intersect(&self.list) {
            self.hits.push((r, hit));
        }
    }
}

/// The panel's column (or a card's inside it), shared by its parts.
#[derive(Clone, Copy)]
struct Column<'a> {
    ds: Ctx<'a>,
    x0: f32,
    w: f32,
}

/// The name's field: the shared field's look (its frame, lit while it has the keys), with
/// the room the studio's rows give it.
fn name_style(ds: &Ctx<'_>) -> TextInputStyle {
    TextInputStyle { padding: ds.px(NAME_PADDING), ..ds::field_style(ds, ds.ui_font(-0.5), false) }
}

/// The room inside the name's field, on each side (logical pixels).
const NAME_PADDING: f32 = 12.0;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
