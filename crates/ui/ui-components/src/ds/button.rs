//! The app's button, one component for every button: what it is ([`ButtonKind`]), its shape,
//! what it shows (an icon, a label, both, the key that presses it), its tone, and how it
//! stands (pressable, under the mouse, turned on, busy). A view never draws a button by
//! hand: it describes it in a [`ButtonSpec`] and calls [`button`] in the rect it laid out
//! (sized by [`button_width`] for one with a label, square for an icon alone), keeping the
//! rect as its hit.
//!
//! The dialogs' kinds (primary, danger, secondary) and a header's (toolbar) are the ones the
//! app always had; ghost (transparent, lit under the mouse: a bar's icon, a toggle like
//! `.*`, a link), soft (a faint fill and border: a small action beside a list) and raised
//! (an opaque card floating over content) cover the rest. What a place needs beyond its
//! kind is an option of the spec: a key cap, a tone filling the back under the mouse, a
//! fractional hover, an icon's own color, a tinted back, a theme's own colors, a surface it
//! stands on, a × of its own ([`chip`]).

use icons::IconId;

use super::*;

/// What kind of button it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// A view's main action: filled in the accent.
    Primary,
    /// An action that only takes away: filled in the danger color.
    Danger,
    /// Beside the main one (Cancel, Browse): bordered, transparent.
    Secondary,
    /// In a view's header row: bordered, an icon then a label, the main one tinted.
    Toolbar,
    /// Transparent, lit under the mouse: a bar's icon, a toggle (`.*`, `Cc`, a pin), a link.
    Ghost,
    /// A faint fill and a border, small: an action beside a list or a card.
    Soft,
    /// Floating over content (a code block's copy, a zoom): an opaque surface with a border
    /// and a light shadow, as a small card.
    Raised,
}

/// Its shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonShape {
    /// Its corners rounded as a button's.
    #[default]
    Box,
    /// A circle (or a pill, wider than high): a close ×, a send arrow.
    Round,
}

/// A theme's own colors for a ghost button (the tool strip's): its icon at rest, its back
/// under the mouse, its back and icon while on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonColors {
    pub icon: Rgba,
    pub hover: Rgba,
    pub on_back: Rgba,
    pub on_icon: Rgba,
}

/// A button described (see the module's doc). Built with [`ButtonSpec::new`] and its
/// setters; what's left out has its default (pressable, not lit, not on, not busy).
#[derive(Debug, Clone, Copy)]
pub struct ButtonSpec<'a> {
    pub kind: ButtonKind,
    pub shape: ButtonShape,
    pub icon: Option<&'a str>,
    /// An icon already looked up (a registered window's): drawn instead of `icon`.
    pub icon_id: Option<IconId>,
    pub label: Option<&'a str>,
    /// Its icon and label in this tone (a Run in green, a Stop in red); `None`: the kind's.
    pub tone: Option<Tone>,
    /// Its tone given as a color (a palette of its own, as the severities' that stay apart
    /// for every eye): wins over [`Self::tone`].
    pub tone_color: Option<Rgba>,
    /// Its icon and label in this tone only while at rest (not lit, not on): a bar's quiet
    /// icon, muted until the mouse is on it.
    pub rest_tone: Option<Tone>,
    /// Turned on (a toggle's state, a header's main action): shown in the accent, a little
    /// stronger under the mouse.
    pub on: bool,
    /// Its action running: a spinner in its icon's place (or before its label), not lit.
    pub busy: bool,
    /// It opens a menu ("Export ▾"): a chevron after its content, counted by
    /// [`button_width`].
    pub menu: bool,
    pub enabled: bool,
    pub hovered: bool,
    /// The key that presses it (`↵`, `esc`), in a small cap after its label, counted by
    /// [`button_width`].
    pub key: Option<&'a str>,
    /// Under the mouse its back is filled in this tone (a window's close ×: solid, its icon
    /// on it in white; a dialog's destructive link: faint, see `hover_solid`).
    pub hover_tone: Option<Tone>,
    /// `hover_tone` fills the back solid (else faint, its content keeping its color).
    pub hover_solid: bool,
    /// How lit it is (0..=1, eased by its caller: an animated hover); `None`: `hovered`'s
    /// all or nothing.
    pub hover_amount: Option<f32>,
    /// Its icon in its own color (a file's tinted icon), its label keeping the kind's.
    pub icon_color: Option<Rgba>,
    /// Its icon's size (logical pixels); `None`: about half its height.
    pub icon_size: Option<f32>,
    /// Its corners' radius (logical pixels); `None`: the kind's.
    pub corner: Option<f32>,
    /// A faint back in its tone's color (the accent when it has none), stronger under the
    /// mouse: a toast's main action.
    pub tinted: bool,
    /// A theme's own colors (a ghost or soft one): they replace the kind's.
    pub colors: Option<ButtonColors>,
    /// The surface it stands on (a fixed dark bar over a canvas): its content in what
    /// reads on it, lit in that color.
    pub surface: Option<Rgba>,
    /// Its label in the dialogs' font ([`button_font`]), beside their filled buttons.
    pub strong: bool,
    /// A × of its own after its content (a filter's pill, see [`chip`]), lit by
    /// `clear_hovered`.
    pub clearable: bool,
    pub clear_hovered: bool,
    /// How far it is on its way to busy (0..=1, eased by its caller): its spinner grows and
    /// fades in, the mouse's light fades out; `None`: `busy`'s all or nothing.
    pub busy_amount: Option<f32>,
    /// Its content in this tone only under the mouse (a delete × turning red), its back as
    /// its kind's.
    pub hover_content_tone: Option<Tone>,
    /// A border in its content's color, faint (a settings row's action, outlined in the
    /// accent or, destructive, in the danger color).
    pub outlined: bool,
    /// Its label in this font (a light button among a bar's texts: the bar's own), over
    /// its kind's.
    pub font: Option<&'a Font>,
    /// Its content from its left edge after this room (logical pixels), not centered: an
    /// item whose value changes in a slot kept for its widest.
    pub start: Option<f32>,
    /// A name's avatar in its icon's place (a project's), as big as an icon would be.
    pub avatar: Option<Avatar<'a>>,
}

impl<'a> ButtonSpec<'a> {
    pub fn new(kind: ButtonKind) -> Self {
        Self {
            kind,
            shape: ButtonShape::Box,
            icon: None,
            icon_id: None,
            label: None,
            tone: None,
            rest_tone: None,
            on: false,
            busy: false,
            menu: false,
            enabled: true,
            hovered: false,
            key: None,
            hover_tone: None,
            hover_solid: false,
            hover_amount: None,
            icon_color: None,
            icon_size: None,
            corner: None,
            tinted: false,
            tone_color: None,
            colors: None,
            surface: None,
            strong: false,
            clearable: false,
            clear_hovered: false,
            busy_amount: None,
            hover_content_tone: None,
            outlined: false,
            font: None,
            start: None,
            avatar: None,
        }
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_id(mut self, icon: IconId) -> Self {
        self.icon_id = Some(icon);
        self
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn tone_color(mut self, color: Rgba) -> Self {
        self.tone_color = Some(color);
        self
    }

    /// Its tone's color, if it has one.
    fn tone_rgba(&self, p: &Palette) -> Option<Rgba> {
        self.tone_color.or_else(|| self.tone.map(|t| t.color(p)))
    }

    pub fn rest_tone(mut self, tone: Tone) -> Self {
        self.rest_tone = Some(tone);
        self
    }

    pub fn round(mut self) -> Self {
        self.shape = ButtonShape::Round;
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn menu(mut self, menu: bool) -> Self {
        self.menu = menu;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    pub fn key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }

    /// Its back filled solid in `tone` under the mouse, its content on it in what reads.
    pub fn hover_tone(mut self, tone: Tone) -> Self {
        self.hover_tone = Some(tone);
        self.hover_solid = true;
        self
    }

    /// Its back faintly in `tone` under the mouse, its content keeping its color.
    pub fn hover_tint(mut self, tone: Tone) -> Self {
        self.hover_tone = Some(tone);
        self.hover_solid = false;
        self
    }

    pub fn hover_amount(mut self, amount: f32) -> Self {
        self.hover_amount = Some(amount.clamp(0.0, 1.0));
        self
    }

    pub fn icon_color(mut self, color: Rgba) -> Self {
        self.icon_color = Some(color);
        self
    }

    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = Some(size);
        self
    }

    pub fn corner(mut self, radius: f32) -> Self {
        self.corner = Some(radius);
        self
    }

    pub fn tinted(mut self) -> Self {
        self.tinted = true;
        self
    }

    pub fn colors(mut self, colors: ButtonColors) -> Self {
        self.colors = Some(colors);
        self
    }

    pub fn on_surface(mut self, surface: Rgba) -> Self {
        self.surface = Some(surface);
        self
    }

    pub fn strong(mut self) -> Self {
        self.strong = true;
        self
    }

    /// A × of its own after its content, lit when `hovered` (its hit: [`clear_rect`]).
    pub fn clearable(mut self, hovered: bool) -> Self {
        self.clearable = true;
        self.clear_hovered = hovered;
        self
    }

    pub fn busy_amount(mut self, amount: f32) -> Self {
        self.busy_amount = Some(amount.clamp(0.0, 1.0));
        self
    }

    pub fn hover_content_tone(mut self, tone: Tone) -> Self {
        self.hover_content_tone = Some(tone);
        self
    }

    /// Outlined in its content's color (see [`Self::outlined`]).
    pub fn outlined(mut self) -> Self {
        self.outlined = true;
        self
    }

    /// Its label in `font` (see [`Self::font`]).
    pub fn font(mut self, font: &'a Font) -> Self {
        self.font = Some(font);
        self
    }

    /// Its content from its left edge after `pad` logical pixels (see [`Self::start`]).
    pub fn start(mut self, pad: f32) -> Self {
        self.start = Some(pad);
        self
    }

    /// `avatar` in its icon's place (see [`Self::avatar`]).
    pub fn avatar(mut self, avatar: Avatar<'a>) -> Self {
        self.avatar = Some(avatar);
        self
    }

    /// How busy it is now (0..=1).
    fn busy_now(&self) -> f32 {
        self.busy_amount.unwrap_or(if self.busy { 1.0 } else { 0.0 })
    }

    /// Busy enough for its spinner to show.
    fn spinning(&self) -> bool {
        self.busy_now() > SPINNER_FROM
    }

    /// How lit it is now (0..=1): never while disabled, fading out as it gets busy.
    fn lit_amount(&self) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let hover = self.hover_amount.unwrap_or(if self.hovered { 1.0 } else { 0.0 });
        hover * (1.0 - self.busy_now())
    }

    /// Lit enough for its content to take its lit color.
    fn lit(&self) -> bool {
        self.lit_amount() >= LIT_AT
    }

    fn has_icon(&self) -> bool {
        self.icon.is_some() || self.icon_id.is_some() || self.avatar.is_some()
    }
}

/// From how lit a fractional hover is its content in its lit color.
const LIT_AT: f32 = 0.5;
/// Below this a fractional busy's spinner would be a speck: none.
const SPINNER_FROM: f32 = 0.05;

/// The sizes a ghost or soft button comes in (logical pixels): a tab's ×, a bar's icon, a
/// view's bar, a compact header's, a roomy bar's, the tool strip's.
pub mod button_size {
    pub const SMALL: f32 = 22.0;
    /// A compact header's icon (a flame graph's back, a form's reset, a calendar's arrows).
    pub const COMPACT: f32 = 24.0;
    pub const MEDIUM: f32 = 26.0;
    pub const LARGE: f32 = 28.0;
    /// A roomy bar's (a header [`super::TOOLBAR_BUTTON_H`] high).
    pub const ROOMY: f32 = 30.0;
    /// The tool strip's, a field's side action (the pipette).
    pub const SPACIOUS: f32 = 32.0;
}

/// An icon's size in a button `height` high: about half of it (15 in 28), never under 12;
/// or the size the spec asks.
fn icon_size(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>, height: f32) -> f32 {
    match spec.icon_size {
        Some(size) => ctx.px(size),
        None => (height * ICON_SHARE).max(ctx.px(MIN_ICON)).round(),
    }
}

/// An icon's share of its button's height, and the smallest an icon is drawn (logical).
const ICON_SHARE: f32 = 15.0 / 28.0;
const MIN_ICON: f32 = 12.0;

/// A menu button's chevron (logical pixels), and the room before it.
const MENU_CHEVRON: f32 = 10.0;
const MENU_GAP: f32 = 4.0;

/// A key cap: its height, the room on each side of its key, its least width, the room before
/// it, its corners (logical pixels).
const KEY_H: f32 = 18.0;
const KEY_PAD: f32 = 4.0;
const KEY_MIN_W: f32 = 18.0;
const KEY_GAP: f32 = 8.0;
const KEY_RADIUS: f32 = 4.0;
/// A key cap's font, below the UI's (points).
const KEY_FONT: f32 = -2.5;

/// A clearable button's ×, and the room before it (logical pixels).
const CLEAR_SIZE: f32 = 16.0;
const CLEAR_GAP: f32 = 4.0;

/// The room around a ghost or soft button's content, and between its icon and its label
/// (logical pixels).
const PAD: f32 = 8.0;
const GAP: f32 = 6.0;

/// The fills of a light button (alphas): a ghost's under the mouse, a soft's at rest and
/// under it, on, on under the mouse, a faint hover tone, a tinted back at rest and under the
/// mouse, a surface's light, a raised one's under the mouse.
const GHOST_HOVER: f32 = 0.08;
const SOFT_REST: f32 = 0.05;
const SOFT_HOVER: f32 = 0.09;
const ON_REST: f32 = 0.14;
const ON_HOVER: f32 = 0.22;
const HOVER_TINT: f32 = 0.08;
const TINT_REST: f32 = 0.15;
const TINT_HOVER: f32 = 0.24;
const SURFACE_HOVER: f32 = 0.14;
const RAISED_HOVER: f32 = 0.06;
/// An outlined button's border (its content's color).
const OUTLINE_ALPHA: f32 = 0.55;

/// The white a filled button's key cap is drawn with: its back, its key.
const FILLED_KEY_BACK: f32 = 0.18;
const FILLED_KEY_TEXT: f32 = 0.85;
/// A plain key cap's back (the text's color).
pub(crate) const KEY_BACK: f32 = 0.06;

/// The room a menu button's chevron adds (physical pixels; none for another button).
fn menu_room(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> f32 {
    if spec.menu { ctx.px(MENU_GAP + MENU_CHEVRON) } else { 0.0 }
}

/// The room a clearable button's × adds (physical pixels).
fn clear_room(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> f32 {
    if spec.clearable { ctx.px(CLEAR_GAP + CLEAR_SIZE) } else { 0.0 }
}

/// A key cap's font.
fn key_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(KEY_FONT)
}

/// A key cap's width for `key` (physical pixels).
fn key_width(text: &mut TextSystem, ctx: &Ctx<'_>, key: &str) -> f32 {
    let w = text.shape_line(key, &key_font(ctx)).width + 2.0 * ctx.px(KEY_PAD);
    w.max(ctx.px(KEY_MIN_W)).round()
}

/// The room a key cap adds (physical pixels; none without a key).
fn key_room(text: &mut TextSystem, ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> f32 {
    spec.key.map_or(0.0, |k| ctx.px(KEY_GAP) + key_width(text, ctx, k))
}

/// What follows a button's label (its key cap, its chevron, its ×).
fn trailing_room(text: &mut TextSystem, ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> f32 {
    key_room(text, ctx, spec) + menu_room(ctx, spec) + clear_room(ctx, spec)
}

/// A square button `size` logical pixels across, centered vertically on `mid`, from `x`.
pub fn square_button(ctx: &Ctx<'_>, (x, mid): (f32, f32), size: f32) -> Rect {
    let s = ctx.px(size);
    Rect::new(x.round(), (mid - s / 2.0).round(), s, s)
}

/// How wide a button with a label is (`height` its own; an icon alone: square), with the
/// room of its key, its menu's chevron and its ×.
pub fn button_width(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    spec: &ButtonSpec<'_>,
    height: f32,
) -> f32 {
    let trailing = trailing_room(text, ctx, spec);
    match (spec.kind, spec.label) {
        (ButtonKind::Primary | ButtonKind::Danger | ButtonKind::Secondary, Some(label)) => {
            primary_width(text, ctx, &[label]) + trailing
        }
        (ButtonKind::Toolbar, Some(label)) => toolbar_button_width(text, ctx, label) + trailing,
        (_, Some(label)) => {
            let font = label_font(ctx, spec);
            let icon =
                if spec.has_icon() { icon_size(ctx, spec, height) + ctx.px(GAP) } else { 0.0 };
            (text.shape_line(label, &font).width + icon + trailing + 2.0 * ctx.px(PAD)).round()
        }
        (_, None) if trailing > 0.0 => (height + trailing).round(),
        (_, None) => height,
    }
}

/// The font of a ghost or soft button's label: what a view measures or cuts a label with
/// ([`ellipsize`]) before giving it to the button.
pub fn button_label_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-1.0).with_weight(BUTTON_WEIGHT)
}

/// A light button's label font: the one it was given, or its own, or the dialogs' when
/// `strong`.
fn label_font(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> Font {
    match spec.font {
        Some(font) => font.clone(),
        None if spec.strong => button_font(ctx),
        None => button_label_font(ctx),
    }
}

/// Where a clearable button's × is in its `rect` (its own hit: a press there clears).
pub fn clear_rect(ctx: &Ctx<'_>, rect: Rect) -> Rect {
    let s = ctx.px(CLEAR_SIZE);
    let x = rect.max_x() - ctx.px(PAD) - s;
    Rect::new(x.round(), (rect.min_y() + (rect.size.height - s) / 2.0).round(), s, s)
}

/// A chip's two parts: its body (opens its menu) and its × (clears it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipRects {
    pub body: Rect,
    pub clear: Rect,
}

/// A filter's chip: a round soft button with its label, its ▾ and a × of its own, in
/// `rect` (from [`button_width`] of the same spec). `spec` says its label, `on` (a filter in
/// force: in the accent), `hovered` (its body) and `clear_hovered`; its menu and × are
/// added. Returns its two hits.
pub fn chip(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) -> ChipRects {
    let spec = ButtonSpec { menu: true, clearable: true, ..spec };
    button(layer, text, icons, ctx, rect, spec);
    let clear = clear_rect(ctx, rect);
    let body = Rect::new(
        rect.min_x(),
        rect.min_y(),
        (clear.min_x() - rect.min_x()).max(0.0),
        rect.size.height,
    );
    ChipRects { body, clear }
}

/// [`chip`]'s spec: a round soft button with its menu and its ×.
pub fn chip_spec<'a>(
    label: &'a str,
    on: bool,
    hovered: bool,
    clear_hovered: bool,
) -> ButtonSpec<'a> {
    ButtonSpec::new(ButtonKind::Soft)
        .label(label)
        .round()
        .on(on)
        .menu(true)
        .hovered(hovered)
        .clearable(clear_hovered)
}

/// Draws the button `spec` describes in `rect`.
impl ButtonSpec<'_> {
    /// What it is to a screen reader: a button named by its label (an icon alone: by the
    /// text on its line, never the icon's own id), greyed out while it can't be used; one
    /// opening a menu says its list is closed.
    fn described(&self, rect: Rect) -> ui_kit::access::Described {
        use ui_kit::access::{Described, Role};
        let name = self.label.unwrap_or_default();
        let mut node = Described::new(Role::Button, name, rect).disabled(!self.enabled);
        node.state.selected = self.on;
        if self.menu {
            node.state.expanded = Some(false);
        }
        node
    }
}

pub fn button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) {
    // Its label is its name: drawn under it, not read a second time.
    ui_kit::access::group(
        || spec.described(rect),
        || paint_button(layer, text, icons, ctx, rect, spec),
    );
}

fn paint_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) {
    let lit = spec.lit();
    let heavy =
        matches!(spec.kind, ButtonKind::Primary | ButtonKind::Danger | ButtonKind::Secondary);
    if spec.shape == ButtonShape::Round
        && matches!(spec.kind, ButtonKind::Primary | ButtonKind::Danger)
    {
        return round_filled(layer, text, icons, ctx, rect, spec);
    }
    // A busy dialog button (primary, danger, secondary) draws its label after its spinner,
    // without its key or chevron (nothing over the spinner).
    let busy_heavy = heavy && spec.busy_now() > 0.0;
    // A menu or keyed button's label leaves its chevron's and key's room (drawn after the
    // back).
    let custom = heavy && !busy_heavy && (spec.menu || spec.key.is_some());
    let label = if custom { "" } else { spec.label.unwrap_or_default() };
    match spec.kind {
        ButtonKind::Primary if busy_heavy => {
            let hovered = spec.hovered && spec.enabled;
            busy_primary_button(layer, text, ctx, rect, label, spec.busy_now(), hovered);
        }
        ButtonKind::Danger | ButtonKind::Secondary if busy_heavy => {
            busy_heavy_button(layer, text, ctx, rect, &spec);
        }
        ButtonKind::Primary => primary_button(layer, text, ctx, rect, label, spec.enabled, lit),
        ButtonKind::Danger => danger_button(layer, text, ctx, rect, label, spec.enabled, lit),
        ButtonKind::Secondary => secondary_button(layer, text, ctx, rect, label, spec.enabled, lit),
        ButtonKind::Toolbar => {
            let icon = spec.icon.unwrap_or_default();
            let toolbar = ToolbarButton { label, icon, accent: spec.on, hovered: lit };
            if spec.busy_now() >= LIT_AT {
                busy_toolbar_button(layer, text, icons, ctx, rect, toolbar);
            } else {
                toolbar_button(layer, text, icons, ctx, rect, toolbar);
            }
        }
        ButtonKind::Ghost | ButtonKind::Soft | ButtonKind::Raised => {
            return light_button(layer, text, icons, ctx, rect, spec);
        }
    }
    if !busy_heavy && (spec.menu || spec.key.is_some()) {
        heavy_trailing(layer, text, icons, ctx, rect, spec);
    }
}

/// A busy danger or secondary button, as [`busy_primary_button`] in its own kind: the
/// danger one filled in its color, the secondary bordered and transparent; the mouse's light
/// fading out as it gets busy, its spinner and label in its content's color.
fn busy_heavy_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: &ButtonSpec<'_>,
) {
    let p = ctx.palette;
    let busy = spec.busy_now();
    let lit = if spec.hovered && spec.enabled { 1.0 - busy } else { 0.0 };
    let radius = ctx.px(radius::MD);
    let color = if spec.kind == ButtonKind::Danger {
        let fill = p.danger.mix(Rgba::WHITE, FILLED_HOVER_MIX * lit);
        layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(radius));
        Rgba::WHITE
    } else {
        let fill = p.text.with_alpha(SECONDARY_HOVER * lit);
        layer.quad(
            Quad::filled(rect, ctx.fade(fill))
                .rounded(radius)
                .bordered(ctx.line(), ctx.fade(p.border)),
        );
        if spec.enabled { p.text } else { p.text_muted }
    };
    busy_label(layer, text, ctx, rect, (spec.label.unwrap_or_default(), color), busy);
}

/// A secondary button's back under the mouse (the text's color).
const SECONDARY_HOVER: f32 = 0.08;

/// A filled, secondary or toolbar button's label (in what its key and chevron leave), its
/// key cap and its chevron, over the back its kind drew.
fn heavy_trailing(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) {
    let p = ctx.palette;
    let lit = spec.lit();
    let filled = matches!(spec.kind, ButtonKind::Primary | ButtonKind::Danger);
    let color = match spec.kind {
        _ if !spec.enabled => p.text_muted,
        ButtonKind::Primary | ButtonKind::Danger => Rgba::WHITE,
        ButtonKind::Toolbar if spec.on => p.accent,
        ButtonKind::Toolbar if !lit => p.text.with_alpha(0.86),
        _ => p.text,
    };
    let menu = menu_room(ctx, &spec);
    let pad = ctx.px(PAD);
    let mid = rect.min_y() + rect.size.height / 2.0;
    let key_w = spec.key.map(|k| (k, key_width(text, ctx, k)));
    if spec.kind == ButtonKind::Toolbar {
        if let Some((key, w)) = key_w {
            let x = rect.max_x() - pad - menu - w;
            key_cap(layer, text, ctx, (x, mid), (key, w), (filled, spec.enabled));
        }
    } else {
        // The label and its key centered together in what the chevron leaves.
        let font = button_font(ctx);
        let shaped = text.shape_line(spec.label.unwrap_or_default(), &font);
        let keyed = key_w.map_or(0.0, |(_, w)| ctx.px(KEY_GAP) + w);
        let room = rect.size.width - menu;
        let x = (rect.min_x() + (room - shaped.width - keyed) / 2.0).round();
        let baseline = optical_baseline(rect.min_y(), rect.size.height, &font);
        draw_text(layer, &shaped, x, baseline, ctx.fade(color));
        if let Some((key, w)) = key_w {
            let kx = x + shaped.width + ctx.px(KEY_GAP);
            key_cap(layer, text, ctx, (kx, mid), (key, w), (filled, spec.enabled));
        }
    }
    if spec.menu {
        chevron(layer, icons, ctx, (rect.max_x() - pad - ctx.px(MENU_CHEVRON), rect), color);
    }
}

/// A key cap `w` wide from `x`, centered on `mid`: white on a filled button, muted on
/// another.
fn key_cap(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (x, mid): (f32, f32),
    (key, w): (&str, f32),
    (filled, enabled): (bool, bool),
) {
    let p = ctx.palette;
    let (back, color) = match (filled, enabled) {
        (true, true) => {
            (Rgba::WHITE.with_alpha(FILLED_KEY_BACK), Rgba::WHITE.with_alpha(FILLED_KEY_TEXT))
        }
        _ => (p.text.with_alpha(KEY_BACK), p.text_muted),
    };
    let h = ctx.px(KEY_H);
    let tile = Rect::new(x.round(), (mid - h / 2.0).round(), w, h);
    let font = key_font(ctx);
    let shaped = text.shape_line(key, &font);
    let look = inline_key_look(ctx, back, color);
    let at = (
        ui_kit::keycap_label_x(tile.min_x(), w, shaped.width),
        optical_baseline(tile.min_y(), h, &font),
    );
    ui_kit::paint_keycap(layer, tile, &shaped, at, &look);
}

/// A key inside a button: a flat tile in `back`, its label in `color`, small corners.
pub fn inline_key_look(ctx: &Ctx<'_>, back: Rgba, color: Rgba) -> ui_kit::KeycapLook {
    ui_kit::KeycapLook::flat(ctx.fade(back), ctx.fade(color), ctx.px(KEY_RADIUS))
}

/// A menu button's chevron from `x`, centered in the button's `rect`.
fn chevron(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    (x, rect): (f32, Rect),
    color: Rgba,
) {
    let Some(id) = icons.id("chevron-down") else { return };
    let size = ctx.px(MENU_CHEVRON);
    let mid = rect.min_y() + rect.size.height / 2.0;
    let r = Rect::new(x.round(), (mid - size / 2.0).round(), size, size);
    ui_kit::draw_icon(layer, icons, id, r, ctx.fade(color));
}

/// A round primary or danger button (a send arrow): a filled circle, its content in white.
fn round_filled(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) {
    let p = ctx.palette;
    let base = if spec.kind == ButtonKind::Danger { p.danger } else { p.accent };
    let fill = if !spec.enabled {
        p.text.with_alpha(GHOST_HOVER)
    } else {
        base.mix(Rgba::WHITE, FILLED_HOVER_MIX * spec.lit_amount())
    };
    let radius = rect.size.width.min(rect.size.height) / 2.0;
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(radius));
    let color = if spec.enabled { Rgba::WHITE } else { p.text_muted };
    content(layer, text, icons, ctx, rect, &spec, (color, spec.enabled));
}

/// How much white a filled button takes under the mouse.
const FILLED_HOVER_MIX: f32 = 0.12;

/// A ghost, soft or raised button: its back (lit, on, the soft fill, the surface), then
/// its content.
fn light_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: ButtonSpec<'_>,
) {
    let p = ctx.palette;
    let radius = match (spec.corner, spec.shape) {
        (Some(r), _) => ctx.px(r),
        (None, ButtonShape::Round) => rect.size.height / 2.0,
        (None, ButtonShape::Box) => ctx.px(radius::SM),
    };
    if spec.kind == ButtonKind::Raised {
        surface_with(layer, ctx, rect, Elevation::Low, radius, p.popup_background);
    }
    let fill = light_fill(ctx, &spec);
    let color = light_color(ctx, &spec);
    let border = match (spec.outlined, spec.kind) {
        (true, _) => Some(color.with_alpha(color.a * OUTLINE_ALPHA)),
        (false, ButtonKind::Soft) => Some(p.border),
        _ => None,
    };
    let mut back = Quad::filled(rect, ctx.fade(fill.unwrap_or(Rgba::TRANSPARENT))).rounded(radius);
    if let Some(border) = border {
        back = back.bordered(ctx.line(), ctx.fade(border));
    }
    if fill.is_some() || border.is_some() {
        layer.quad(back);
    }
    content(layer, text, icons, ctx, rect, &spec, (color, false));
}

/// A light button's back now (`None`: none).
fn light_fill(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> Option<Rgba> {
    let p = ctx.palette;
    let amount = spec.lit_amount();
    let lerp = |rest: f32, hover: f32| rest + (hover - rest) * amount;
    let soft = spec.kind == ButtonKind::Soft;
    if spec.on {
        return Some(match spec.colors {
            Some(c) => c.on_back,
            None => p.accent.with_alpha(lerp(ON_REST, ON_HOVER)),
        });
    }
    if let (Some(tone), true) = (spec.hover_tone, amount > 0.0) {
        let color = tone.color(p);
        let alpha = if spec.hover_solid { color.a } else { HOVER_TINT };
        return Some(color.with_alpha(alpha * amount));
    }
    if spec.tinted {
        let color = spec.tone_rgba(p).unwrap_or(p.accent);
        return Some(color.with_alpha(lerp(TINT_REST, TINT_HOVER)));
    }
    if let Some(c) = spec.colors {
        return (amount > 0.0).then(|| c.hover.with_alpha(c.hover.a * amount));
    }
    if let Some(surface) = spec.surface {
        return (amount > 0.0).then(|| sign_on(surface).with_alpha(SURFACE_HOVER * amount));
    }
    match spec.kind {
        ButtonKind::Raised => (amount > 0.0).then(|| p.text.with_alpha(RAISED_HOVER * amount)),
        _ if soft => Some(p.text.with_alpha(lerp(SOFT_REST, SOFT_HOVER))),
        _ => (amount > 0.0).then(|| p.text.with_alpha(GHOST_HOVER * amount)),
    }
}

/// A light button's content color now.
fn light_color(ctx: &Ctx<'_>, spec: &ButtonSpec<'_>) -> Rgba {
    let p = ctx.palette;
    let lit = spec.lit();
    if !spec.enabled {
        return p.text_muted.with_alpha(0.5);
    }
    if let (Some(tone), true, true) = (spec.hover_tone, spec.hover_solid, lit) {
        return sign_on(tone.color(p));
    }
    if let (Some(tone), true) = (spec.hover_content_tone, lit) {
        return tone.color(p);
    }
    if let Some(c) = spec.colors {
        return if spec.on { c.on_icon } else { c.icon };
    }
    if let Some(color) = spec.tone_rgba(p) {
        return color;
    }
    if spec.tinted || spec.on {
        return p.accent;
    }
    if let Some(surface) = spec.surface {
        return sign_on(surface);
    }
    match (spec.rest_tone, lit) {
        (Some(tone), false) => tone.color(p),
        _ => p.text,
    }
}

/// A button's content centered in `rect`: its spinner or icon, its label, its key cap, its
/// chevron, its ×, in `color` (its key cap white when `filled`).
fn content(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    spec: &ButtonSpec<'_>,
    (color, filled): (Rgba, bool),
) {
    let icon = icon_size(ctx, spec, rect.size.height);
    let font = label_font(ctx, spec);
    let shaped = spec.label.map(|l| text.shape_line(l, &font));
    let label_w = shaped.as_ref().map_or(0.0, |s| s.width);
    let busy = spec.busy_now();
    let has_icon = spec.has_icon() || busy > 0.0;
    let key_w = spec.key.map(|k| (k, key_width(text, ctx, k)));
    let content = if has_icon { icon } else { 0.0 }
        + if has_icon && shaped.is_some() { ctx.px(GAP) } else { 0.0 }
        + label_w
        + key_room(text, ctx, spec)
        + menu_room(ctx, spec);
    // The × keeps its own place at the end; the rest is centered in what it leaves.
    let room = rect.size.width - clear_room(ctx, spec);
    let x0 = match spec.start {
        Some(pad) => (rect.min_x() + ctx.px(pad)).round(),
        None => (rect.min_x() + (room - content) / 2.0).round(),
    };
    let mid = rect.min_y() + rect.size.height / 2.0;
    // Where the last piece drawn ends.
    let mut end = x0;
    if spec.spinning() {
        let spinner = Loader {
            size: icon / ctx.scale * busy,
            color: Some(color.with_alpha(color.a * busy)),
            ..Loader::default()
        };
        // Grown from its middle: the room it takes stays the icon's.
        loader(layer, text, ctx, x0 + icon * (1.0 - busy) / 2.0, mid, &spinner);
        end = x0 + icon;
    } else if busy > 0.0 && !spec.has_icon() {
        end = x0 + icon;
    } else if let Some(avatar) = spec.avatar {
        let r = Rect::new(x0, (mid - icon / 2.0).round(), icon, icon);
        avatar.paint(layer, text, ctx, r);
        end = x0 + icon;
    } else if let Some(id) = spec.icon_id.or_else(|| spec.icon.and_then(|i| icons.id(i))) {
        let r = Rect::new(x0, (mid - icon / 2.0).round(), icon, icon);
        let tint = match spec.icon_color {
            Some(c) if spec.enabled => c,
            _ => color,
        };
        ui_kit::draw_icon(layer, icons, id, r, ctx.fade(tint));
        end = x0 + icon;
    } else if has_icon {
        end = x0 + icon;
    }
    if let Some(shaped) = shaped {
        let x = if has_icon { end + ctx.px(GAP) } else { end };
        let baseline = optical_baseline(rect.min_y(), rect.size.height, &font);
        draw_text(layer, &shaped, x.round(), baseline, ctx.fade(color));
        end = x + label_w;
    }
    if let Some((key, w)) = key_w {
        let x = end + ctx.px(KEY_GAP);
        key_cap(layer, text, ctx, (x, mid), (key, w), (filled, spec.enabled));
        end = x + w;
    }
    if spec.menu {
        chevron(layer, icons, ctx, (end + ctx.px(MENU_GAP), rect), color);
    }
    if spec.clearable {
        let r = clear_rect(ctx, rect);
        let s = r.size.width;
        if spec.clear_hovered && spec.enabled {
            let lit = color.with_alpha(GHOST_HOVER * 2.0);
            layer.quad(Quad::filled(r, ctx.fade(lit)).rounded(s / 2.0));
        }
        if let Some(id) = icons.id("x") {
            let glyph = (s * CLEAR_GLYPH_SHARE).round();
            let at = Rect::new(
                (r.min_x() + (s - glyph) / 2.0).round(),
                (r.min_y() + (s - glyph) / 2.0).round(),
                glyph,
                glyph,
            );
            ui_kit::draw_icon(layer, icons, id, at, ctx.fade(color));
        }
    }
}

/// The × glyph's share of its round hit.
const CLEAR_GLYPH_SHARE: f32 = 0.75;
