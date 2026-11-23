//! Badges and labelled rows: a tone's badge, a name's avatar, a dot on a button, a file's
//! row, a key cap.

use super::*;

/// What [`badge`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct BadgeSpec<'a> {
    pub label: &'a str,
    pub x: f32,
    pub mid: f32,
    pub align_right: bool,
    pub tone: Tone,
}

/// A rounded badge with small text (e.g. `Linia 11`, `semantic`), vertically centered on
/// `mid`; `x` is the right edge if `align_right`. Returns the rectangle.
pub fn badge(layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, spec: BadgeSpec<'_>) -> Rect {
    let BadgeSpec { label, x, mid, align_right, tone } = spec;
    let badge = Badge::new(label).tone(tone);
    if align_right {
        badge.paint_right(layer, text, ctx, x, mid)
    } else {
        badge.paint(layer, text, ctx, x, mid)
    }
}

/// A badge's height, by name (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BadgeSize {
    /// 15: a mark after a tab's title.
    Tiny,
    /// 16: a card's tag.
    Small,
    /// 18: a count beside a row's label.
    Medium,
    /// 20: the usual.
    #[default]
    Regular,
    /// A height of the caller's (a pill fitted to a table's row).
    Custom(f32),
}

impl BadgeSize {
    /// Its height (logical pixels).
    pub fn height(self) -> f32 {
        match self {
            Self::Tiny => 15.0,
            Self::Small => 16.0,
            Self::Medium => 18.0,
            Self::Regular => 20.0,
            Self::Custom(h) => h,
        }
    }
}

/// Where a badge's text sits on its height.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BadgeBaseline {
    /// Optically centered (by the cap height: [`optical_baseline`]).
    #[default]
    Optical,
    /// Centered by the font's ascent and descent.
    Metrics,
    /// On the caller's baseline (a pill among a row's words: the row's).
    At(f32),
}

/// The usual badge's padding on each side, its fill's alpha in a tone and in neutral, and
/// its font (relative to the UI's).
const BADGE_PAD: f32 = 7.0;
const BADGE_FILL: f32 = 0.12;
const BADGE_NEUTRAL_FILL: f32 = 0.05;
const BADGE_FONT: f32 = -1.5;

/// A badge's icon's side by default, and the room between its parts (the icon, the muted
/// run, the label), in logical pixels.
const BADGE_ICON: f32 = 12.0;
const BADGE_PART_GAP: f32 = 4.0;

/// The icon before a badge's text (a lock before "Read-only"): which, how big, its color.
#[derive(Clone, Copy)]
pub struct BadgeIcon<'a> {
    pub icons: &'a icons::Icons,
    pub name: &'a str,
    /// Its side (logical pixels).
    pub size: f32,
    /// Its color; `None`: the label's.
    pub color: Option<Rgba>,
}

impl std::fmt::Debug for BadgeIcon<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BadgeIcon")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("color", &self.color)
            .finish_non_exhaustive()
    }
}

/// A badge, as configured: a rounded pill with a small label in a tone. Its defaults are
/// [`badge`]'s; each option changes only what a place asks for. Sizes in logical pixels,
/// times [`Self::zoom`].
///
/// ```ignore
/// Badge::new("3").size(BadgeSize::Medium).centered().min_width(18.0).paint_right(…);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Badge<'a> {
    pub label: &'a str,
    pub tone: Tone,
    /// The text's color, and its fill's base, over the tone's.
    pub color: Option<Rgba>,
    /// The fill's own color, over the one made from the color.
    pub fill: Option<Rgba>,
    /// The fill's alpha over the color (by default 0.12; 0.05 of the text in neutral).
    pub fill_alpha: Option<f32>,
    pub size: BadgeSize,
    /// The room on each side of the label.
    pub pad: f32,
    /// The narrowest it gets (a count's round pill).
    pub min_width: f32,
    /// The corners' radius; `None`: half the height (a pill).
    pub radius: Option<f32>,
    /// The label centered (a count), else from the left after the padding.
    pub centered: bool,
    /// The label in bold.
    pub bold: bool,
    /// The label's font size (relative to the UI's).
    pub font_delta: f32,
    /// The label's font, over the one made from the size and the weight.
    pub font: Option<&'a Font>,
    pub baseline: BadgeBaseline,
    /// What its sizes are multiplied by (a bar zoomed apart from the UI).
    pub zoom: f32,
    /// An outline in this color, a line wide (a chip over a card); `None`: none.
    pub border: Option<Rgba>,
    /// An icon before its text; `None`: none.
    pub icon: Option<BadgeIcon<'a>>,
    /// A muted run before the label (a figure's name before its value: `rows 1 204`).
    pub lead: Option<&'a str>,
    /// The muted run's font; `None`: the label's.
    pub lead_font: Option<&'a Font>,
}

impl<'a> Badge<'a> {
    /// The usual badge (neutral, 20 high).
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            tone: Tone::Neutral,
            color: None,
            fill: None,
            fill_alpha: None,
            size: BadgeSize::Regular,
            pad: BADGE_PAD,
            min_width: 0.0,
            radius: None,
            centered: false,
            bold: false,
            font_delta: BADGE_FONT,
            font: None,
            baseline: BadgeBaseline::Optical,
            zoom: 1.0,
            border: None,
            icon: None,
            lead: None,
            lead_font: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Its text in `color`, the fill made from it.
    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }

    /// Its fill in `fill`.
    pub fn fill(mut self, fill: Rgba) -> Self {
        self.fill = Some(fill);
        self
    }

    /// Its fill at `alpha` of its color.
    pub fn fill_alpha(mut self, alpha: f32) -> Self {
        self.fill_alpha = Some(alpha);
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    /// The room on each side of the label.
    pub fn pad(mut self, pad: f32) -> Self {
        self.pad = pad;
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// The label centered (a count).
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// The label's font size (relative to the UI's).
    pub fn font_delta(mut self, delta: f32) -> Self {
        self.font_delta = delta;
        self
    }

    /// The label in `font`.
    pub fn font(mut self, font: &'a Font) -> Self {
        self.font = Some(font);
        self
    }

    pub fn baseline(mut self, baseline: BadgeBaseline) -> Self {
        self.baseline = baseline;
        self
    }

    /// Its sizes times `zoom`.
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }

    /// Outlined in `color`, a line wide.
    pub fn border(mut self, color: Rgba) -> Self {
        self.border = Some(color);
        self
    }

    /// Icon `name` of `icons` before its text, 12 high, in the label's color.
    pub fn icon(mut self, icons: &'a icons::Icons, name: &'a str) -> Self {
        self.icon = Some(BadgeIcon { icons, name, size: BADGE_ICON, color: None });
        self
    }

    /// Its icon `size` high (logical pixels).
    pub fn icon_size(mut self, size: f32) -> Self {
        if let Some(icon) = &mut self.icon {
            icon.size = size;
        }
        self
    }

    /// Its icon in `color`, apart from the label's.
    pub fn icon_color(mut self, color: Rgba) -> Self {
        if let Some(icon) = &mut self.icon {
            icon.color = Some(color);
        }
        self
    }

    /// A muted run `lead` before the label.
    pub fn lead(mut self, lead: &'a str) -> Self {
        self.lead = Some(lead);
        self
    }

    /// The muted run in `font`.
    pub fn lead_font(mut self, font: &'a Font) -> Self {
        self.lead_font = Some(font);
        self
    }

    /// A logical size at the context's scale and the zoom (physical pixels).
    fn px(&self, ctx: &Ctx<'_>, v: f32) -> f32 {
        ctx.px(v * self.zoom)
    }

    /// The label's font.
    fn label_font(&self, ctx: &Ctx<'_>) -> Font {
        if let Some(font) = self.font {
            return font.clone();
        }
        let font = ctx.ui_font(self.font_delta);
        let font = if self.zoom == 1.0 {
            font
        } else {
            let size = ctx.px((ctx.palette.ui_font_size + self.font_delta) * self.zoom);
            Font::new(font.family, size)
        };
        if self.bold { font.with_weight(Font::BOLD) } else { font }
    }

    /// Its width (physical pixels, whole).
    pub fn width(&self, text: &mut TextSystem, ctx: &Ctx<'_>) -> f32 {
        let font = self.label_font(ctx);
        let label = text.shape_line(self.label, &font).width;
        let lead = self.lead_shaped(text, &font).map_or(0.0, |l| l.width);
        self.width_of(label + lead + self.extras(ctx), ctx).round()
    }

    /// Its whole width around content `inner` wide (physical pixels).
    fn width_of(&self, inner: f32, ctx: &Ctx<'_>) -> f32 {
        (inner + self.px(ctx, 2.0 * self.pad)).max(self.px(ctx, self.min_width))
    }

    /// The room its icon and its muted run take beside their text (physical pixels).
    fn extras(&self, ctx: &Ctx<'_>) -> f32 {
        let icon = self.icon.map_or(0.0, |i| self.px(ctx, i.size + BADGE_PART_GAP));
        let lead = if self.lead.is_some() { self.px(ctx, BADGE_PART_GAP) } else { 0.0 };
        icon + lead
    }

    /// The muted run shaped (in its font, else the label's).
    fn lead_shaped(
        &self,
        text: &mut TextSystem,
        font: &Font,
    ) -> Option<std::sync::Arc<text_layout::ShapedLine>> {
        let lead = self.lead?;
        Some(text.shape_line(lead, self.lead_font.unwrap_or(font)))
    }

    /// Its fill and its text's color.
    fn colors(&self, p: &Palette) -> (Rgba, Rgba) {
        let (fill, color) = match (self.color, self.tone) {
            (Some(c), _) => (c.with_alpha(self.fill_alpha.unwrap_or(BADGE_FILL)), c),
            (None, Tone::Neutral) => {
                (p.text.with_alpha(self.fill_alpha.unwrap_or(BADGE_NEUTRAL_FILL)), p.text_muted)
            }
            (None, t) => (t.color(p).with_alpha(self.fill_alpha.unwrap_or(BADGE_FILL)), t.color(p)),
        };
        (self.fill.unwrap_or(fill), color)
    }

    /// Drawn from `left`, centered on `mid`. Returns its rectangle.
    pub fn paint(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        left: f32,
        mid: f32,
    ) -> Rect {
        self.paint_at(layer, text, ctx, (left, false), mid)
    }

    /// Drawn ending at `right`, centered on `mid`. Returns its rectangle.
    pub fn paint_right(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        right: f32,
        mid: f32,
    ) -> Rect {
        self.paint_at(layer, text, ctx, (right, true), mid)
    }

    fn paint_at(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        (x, align_right): (f32, bool),
        mid: f32,
    ) -> Rect {
        let font = self.label_font(ctx);
        let shaped = text.shape_line(self.label, &font);
        let lead = self.lead_shaped(text, &font);
        let inner = shaped.width + lead.as_ref().map_or(0.0, |l| l.width) + self.extras(ctx);
        let w = self.width_of(inner, ctx);
        let h = self.px(ctx, self.size.height());
        let left = if align_right { x - w } else { x };
        let rect = Rect::new(left.round(), (mid - h / 2.0).round(), w.round(), h);
        let (fill, color) = self.colors(ctx.palette);
        let r = self.radius.map_or(h / 2.0, |v| self.px(ctx, v));
        let quad = Quad::filled(rect, ctx.fade(fill)).rounded(r);
        layer.quad(match self.border {
            Some(border) => quad.bordered(ctx.line(), ctx.fade(border)),
            None => quad,
        });
        let mut tx = if self.centered {
            (rect.min_x() + (rect.size.width - inner) / 2.0).round()
        } else {
            rect.min_x() + self.px(ctx, self.pad)
        };
        if let Some(icon) = self.icon {
            let side = self.px(ctx, icon.size);
            let mid = rect.min_y() + h / 2.0;
            if let Some(id) = icon.icons.id(icon.name) {
                let at = Rect::new(tx.round(), (mid - side / 2.0).round(), side, side);
                ui_kit::draw_icon(layer, icon.icons, id, at, ctx.fade(icon.color.unwrap_or(color)));
            }
            tx += side + self.px(ctx, BADGE_PART_GAP);
        }
        let base = match self.baseline {
            BadgeBaseline::Optical => optical_baseline(rect.min_y(), h, &font),
            BadgeBaseline::Metrics => {
                ui_kit::centered_baseline(rect.min_y(), h, &text.metrics(&font))
            }
            BadgeBaseline::At(base) => base,
        };
        if let Some(lead) = &lead {
            draw_text(layer, lead, tx.round(), base, ctx.fade(ctx.palette.text_muted));
            tx += lead.width + self.px(ctx, BADGE_PART_GAP);
        }
        // Rounded only after parts before it (the label alone stays where it always was).
        let tx = if self.icon.is_some() || lead.is_some() { tx.round() } else { tx };
        draw_text(layer, &shaped, tx, base, ctx.fade(color));
        rect
    }
}

/// How an [`Avatar`] looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarLook {
    /// A circle faintly in its color, the initials in it (a picker's row).
    #[default]
    Tinted,
    /// A rounded square filled with its color, the initials in white, bold (a project's).
    Solid,
}

/// A name's avatar: its initials ([`initials`]) on its color, in a square rect. The one
/// avatar of the app: a picker's rows, the projects, the top bar's project (a button's
/// [`super::ButtonSpec::avatar`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Avatar<'a> {
    pub name: &'a str,
    pub color: Rgba,
    pub look: AvatarLook,
}

/// A tinted avatar's fill (its color's alpha), and its letters' font (below the UI's).
const AVATAR_TINT: f32 = 0.16;
const AVATAR_FONT: f32 = -4.0;
/// A solid avatar's corners (logical pixels), and its letters' share of its height.
const AVATAR_RADIUS: f32 = 5.0;
const AVATAR_LETTERS: f32 = 0.5;

impl<'a> Avatar<'a> {
    /// `name`'s avatar in `color`, tinted.
    pub fn new(name: &'a str, color: Rgba) -> Self {
        Self { name, color, look: AvatarLook::Tinted }
    }

    /// A rounded square filled with its color, white bold letters.
    pub fn solid(mut self) -> Self {
        self.look = AvatarLook::Solid;
        self
    }

    /// Drawn in `rect` (square).
    pub fn paint(&self, layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, rect: Rect) {
        let side = rect.size.height;
        let (font, letters) = match self.look {
            AvatarLook::Tinted => {
                let back = Quad::filled(rect, ctx.fade(self.color.with_alpha(AVATAR_TINT)));
                layer.quad(back.rounded(side / 2.0));
                (ctx.ui_font(AVATAR_FONT).with_weight(Font::SEMIBOLD), self.color)
            }
            AvatarLook::Solid => {
                let back = Quad::filled(rect, ctx.fade(self.color));
                layer.quad(back.rounded(ctx.px(AVATAR_RADIUS).round()));
                let size = (side * AVATAR_LETTERS).round();
                (Font::new(ctx.palette.ui_font.clone(), size).with_weight(Font::BOLD), Rgba::WHITE)
            }
        };
        let shaped = text.shape_line(&initials(self.name), &font);
        let baseline = match self.look {
            AvatarLook::Tinted => optical_baseline(rect.min_y(), side, &font),
            AvatarLook::Solid => {
                ui_kit::centered_baseline(rect.min_y(), side, &text.metrics(&font))
            }
        };
        let x = (rect.min_x() + (rect.size.width - shaped.width) / 2.0).round();
        // Initials are a picture of the name, not text to read (the name is said beside it).
        ui_kit::access::silent(|| draw_text(layer, &shaped, x, baseline, ctx.fade(letters)));
    }
}

/// What a count means: its color by meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeKind {
    /// Something to do (changes, uploads): the accent.
    #[default]
    Count,
    /// Something to look at: the warning color.
    Warning,
    /// Something failed: the error color.
    Error,
}

/// A dot on a button's icon corner (see [`dot_badge`]): there is something (files to
/// upload, changes), its count in the button's tooltip.
#[derive(Debug, Clone, Copy)]
pub struct DotBadge {
    pub kind: BadgeKind,
    /// The button is lit with the accent: the dot white.
    pub lit: bool,
    /// What is behind the button: the ring that sets it apart from the icon.
    pub outline: Rgba,
}

/// A dot badge's diameter, logical pixels.
pub const DOT_BADGE: f32 = 9.0;

/// A small dot centered on an icon's top right corner `at`, its color by meaning, with a
/// 2 px ring in what's behind (`outline`) so it reads cut out of the icon; on whole pixels,
/// scaled with the zoom. Returns its rectangle (without the ring).
pub fn dot_badge(layer: &mut Layer, ctx: &Ctx<'_>, at: Point, badge: DotBadge) -> Rect {
    let DotBadge { kind, lit, outline } = badge;
    let p = ctx.palette;
    let tone = match kind {
        BadgeKind::Count => p.accent,
        BadgeKind::Warning => crate::severity::Level::Warning.color(p),
        BadgeKind::Error => crate::severity::Level::Critical.color(p),
    };
    let d = ctx.px(DOT_BADGE);
    let ring = ctx.px(2.0).max(1.0);
    let rect = Rect::new((at.x - d / 2.0).round(), (at.y - d / 2.0).round(), d, d);
    let outer = Rect::new(rect.min_x() - ring, rect.min_y() - ring, d + 2.0 * ring, d + 2.0 * ring);
    layer.quad(Quad::filled(outer, outline).rounded(outer.size.height / 2.0));
    layer.quad(Quad::filled(rect, if lit { Rgba::WHITE } else { tone }).rounded(d / 2.0));
    rect
}

/// A plain round dot of `diameter` (logical pixels) centered on `center`, in `color` (the
/// current branch, a lane's node, a state): on whole pixels, scaled with the zoom. Returns its
/// rectangle.
pub fn dot(layer: &mut Layer, ctx: &Ctx<'_>, center: Point, diameter: f32, color: Rgba) -> Rect {
    let d = ctx.px(diameter);
    let rect = Rect::new((center.x - d / 2.0).round(), (center.y - d / 2.0).round(), d, d);
    layer.quad(Quad::filled(rect, ctx.fade(color)).rounded(d / 2.0));
    rect
}

/// [`dot`] cut out of what it stands on (an icon's corner): ringed `ring` logical pixels
/// wide in each of `behind`, drawn in order (the surface, then a row's light over it).
/// Returns the dot's rectangle (without the ring).
pub fn ringed_dot(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    center: Point,
    (diameter, ring): (f32, f32),
    color: Rgba,
    behind: &[Rgba],
) -> Rect {
    for back in behind {
        dot(layer, ctx, center, diameter + 2.0 * ring, *back);
    }
    dot(layer, ctx, center, diameter, color)
}

/// A file in a list (what's to upload, what changed): its state's mark (`M`, `A`, `!`…) in
/// its color, its name, its folder dimmed (cut in the middle), and a tag on the right (why
/// it's there), all cut with "…" to fit.
#[derive(Debug, Clone, Copy)]
pub struct FileRow<'a> {
    /// The path, relative (`src/app/Main.php`), `/`-separated.
    pub path: &'a str,
    pub mark: &'a str,
    pub color: Rgba,
    pub tag: Option<(&'a str, Tone)>,
}

/// Paints `row` in `rect` (the row without its buttons), vertically centered.
pub fn file_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    row: FileRow<'_>,
) {
    let (font, small) = (ctx.ui_font(0.0), ctx.ui_font(-1.0));
    let (top, h) = (rect.min_y(), rect.size.height);
    let mark = text.shape_line(row.mark, &small.clone().with_weight(Font::SEMIBOLD));
    let mark_x = rect.min_x() + ctx.px(4.0);
    draw_text(layer, &mark, mark_x, optical_baseline(top, h, &small), row.color);
    let mut right = rect.max_x();
    if let Some((tag, tone)) = row.tag {
        let tag = ellipsize(text, tag, &ctx.ui_font(-1.5), (rect.size.width * 0.4).max(0.0));
        if !tag.is_empty() {
            let mid = top + h / 2.0;
            let spec = BadgeSpec { label: &tag, x: right, mid, align_right: true, tone };
            right = badge(layer, text, ctx, spec).min_x() - ctx.px(space::MD);
        }
    }
    let (folder, name) = row.path.rsplit_once('/').unwrap_or(("", row.path));
    let x = rect.min_x() + ctx.px(26.0);
    let name = ellipsize(text, name, &font, (right - x).max(0.0));
    let shaped = text.shape_line(&name, &font);
    draw_text(layer, &shaped, x, optical_baseline(top, h, &font), ctx.palette.text);
    let fx = x + shaped.width + ctx.px(space::MD);
    if !folder.is_empty() && right - fx > ctx.px(24.0) {
        let folder = ellipsize_middle(text, folder, &small, right - fx);
        let shaped = text.shape_line(&folder, &small);
        draw_text(layer, &shaped, fx, optical_baseline(top, h, &small), ctx.palette.text_muted);
    }
}

/// A single key (e.g. `esc` in a field's corner), vertically centered on `mid`, right-aligned
/// at `right`. Returns the rectangle.
pub fn keycap(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    key: &str,
    right: f32,
    mid: f32,
    hovered: bool,
) -> Rect {
    Keycap::new(key).hovered(hovered).paint_right(layer, text, ctx, right, mid)
}

/// The width [`keycap`] draws `key` at (physical pixels), to place it.
pub fn keycap_width(text: &mut TextSystem, ctx: &Ctx<'_>, key: &str) -> f32 {
    Keycap::new(key).width(text, ctx)
}

/// A key cap's size, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeycapSize {
    /// 26 high, at least 28 wide: a field's or a modal's corner (`esc`).
    #[default]
    Regular,
    /// 20 high, at least 22 wide: a key in a dense row.
    Small,
}

impl KeycapSize {
    /// Its height, its least width, the room around its label (logical pixels) and its
    /// font (relative to the UI's).
    fn metrics(self) -> (f32, f32, f32, f32) {
        match self {
            Self::Regular => (26.0, 28.0, 16.0, -2.0),
            Self::Small => (20.0, 22.0, 12.0, -2.5),
        }
    }
}

/// A key cap in a field's corner, as configured: its key, lit under the mouse, its size.
/// Drawn by the one key cap ([`ui_kit::paint_keycap`]).
#[derive(Debug, Clone, Copy)]
pub struct Keycap<'a> {
    pub key: &'a str,
    pub hovered: bool,
    pub size: KeycapSize,
}

/// A field's key cap under the mouse: its fill's alpha (of the text) and its outline's.
const KEYCAP_HOVER_FILL: f32 = 0.06;
const KEYCAP_HOVER_BORDER: f32 = 0.5;

impl<'a> Keycap<'a> {
    pub fn new(key: &'a str) -> Self {
        Self { key, hovered: false, size: KeycapSize::Regular }
    }

    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    pub fn size(mut self, size: KeycapSize) -> Self {
        self.size = size;
        self
    }

    fn font(&self, ctx: &Ctx<'_>) -> Font {
        ctx.ui_font(self.size.metrics().3)
    }

    /// Its width (physical pixels).
    pub fn width(&self, text: &mut TextSystem, ctx: &Ctx<'_>) -> f32 {
        let (_, least, room, _) = self.size.metrics();
        (text.shape_line(self.key, &self.font(ctx)).width + ctx.px(room)).max(ctx.px(least))
    }

    /// Its look: the popup's fill and the border at rest, lit under the mouse.
    pub fn look(&self, ctx: &Ctx<'_>) -> ui_kit::KeycapLook {
        let p = ctx.palette;
        let (fill, border, label) = if self.hovered {
            (
                p.text.with_alpha(KEYCAP_HOVER_FILL),
                p.text_muted.with_alpha(KEYCAP_HOVER_BORDER),
                p.text,
            )
        } else {
            (p.popup_background, p.border, p.text_muted)
        };
        ui_kit::KeycapLook {
            background: ctx.fade(fill),
            border: Some(ui_kit::KeycapBorder { width: ctx.line(), color: ctx.fade(border) }),
            relief: None,
            radius: ctx.px(radius::SM),
            text: ctx.fade(label),
        }
    }

    /// Drawn ending at `right`, centered on `mid`. Returns its rectangle.
    pub fn paint_right(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        right: f32,
        mid: f32,
    ) -> Rect {
        let w = self.width(text, ctx);
        self.paint_at(layer, text, ctx, right - w, mid, w)
    }

    /// Drawn from `left`, centered on `mid`. Returns its rectangle.
    pub fn paint(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        left: f32,
        mid: f32,
    ) -> Rect {
        let w = self.width(text, ctx);
        self.paint_at(layer, text, ctx, left, mid, w)
    }

    fn paint_at(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        left: f32,
        mid: f32,
        w: f32,
    ) -> Rect {
        let font = self.font(ctx);
        let shaped = text.shape_line(self.key, &font);
        let h = ctx.px(self.size.metrics().0);
        let rect = Rect::new(left.round(), (mid - h / 2.0).round(), w.round(), h);
        let base = optical_baseline(rect.min_y(), h, &font);
        let x = ui_kit::keycap_label_x(rect.min_x(), w, shaped.width);
        ui_kit::paint_keycap(layer, rect, &shaped, (x, base), &self.look(ctx));
        rect
    }
}

/// A key drawn over a picture (About's illustration, a full-window scene): a dark,
/// translucent chip — night blue, a thin light border, near-white label — readable over
/// anything, a bright sky included. Its top-right corner at `top_right` (logical sizes:
/// 22 high, the label 11 with 8 on each side). Returns the rectangle.
pub fn overlay_key(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    shaped: &text_layout::ShapedLine,
    font: &Font,
    top_right: Point,
) -> Rect {
    let px = |v: f32| ctx.px(v);
    let h = px(22.0);
    let w = shaped.width + px(16.0);
    let rect = Rect::new((top_right.x - w).round(), top_right.y.round(), w.round(), h);
    layer.quad(
        Quad::filled(rect, ctx.fade(OVERLAY_KEY_FILL))
            .rounded(px(6.0))
            .bordered(ctx.line(), ctx.fade(Rgba::WHITE.with_alpha(0.28))),
    );
    draw_text(
        layer,
        shaped,
        rect.min_x() + px(8.0),
        optical_baseline(rect.min_y(), h, font),
        ctx.fade(Rgba::WHITE.with_alpha(0.95)),
    );
    rect
}

/// [`overlay_key`]'s fill: night blue, 62 %.
pub const OVERLAY_KEY_FILL: Rgba = Rgba::rgb(0x0d0f24).with_alpha(0.62);

/// [`overlay_key`]'s label font at `ctx`'s scale.
pub fn overlay_key_font(ctx: &Ctx<'_>) -> Font {
    Font::new(ctx.palette.ui_font.clone(), ctx.px(11.0))
}

/// A corner of an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

/// What [`glass_badge`] draws: an icon on its tile, a title and what it's about, on a pane of
/// frosted glass in a corner. Not a notification: it only says what's on screen.
#[derive(Debug, Clone, Copy)]
pub struct GlassBadge<'a> {
    /// The icon's name in the app's set.
    pub icon: &'a str,
    /// A picture shown instead of the icon, larger and on no tile (the app's logo).
    pub image: Option<&'a render_api::ImageData>,
    pub title: &'a str,
    /// The smaller line under the title.
    pub detail: Option<&'a str>,
    pub corner: Corner,
    /// How far from the corner (logical pixels).
    pub margin: f32,
    /// What the glass and the words are tinted with (light over a dark screen).
    pub tint: Rgba,
    /// How much of it shows (0..1).
    pub opacity: f32,
}

impl<'a> GlassBadge<'a> {
    /// A light pane in the bottom right corner.
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            image: None,
            title,
            detail: None,
            corner: Corner::default(),
            margin: 24.0,
            tint: Rgba::WHITE,
            opacity: 1.0,
        }
    }
}

/// Where a pane of `size` goes in `area`, `margin` from `corner`.
pub fn in_corner(area: Rect, size: (f32, f32), corner: Corner, margin: f32) -> Point {
    let (left, top) = (area.min_x() + margin, area.min_y() + margin);
    let (right, bottom) = (area.max_x() - margin - size.0, area.max_y() - margin - size.1);
    match corner {
        Corner::TopLeft => Point::new(left, top),
        Corner::TopRight => Point::new(right, top),
        Corner::BottomLeft => Point::new(left, bottom),
        Corner::BottomRight => Point::new(right, bottom),
    }
}

/// A glass badge's picture (its side), padding and the gap after the picture.
fn glass_badge_parts(ctx: &Ctx<'_>, badge: &GlassBadge<'_>) -> (f32, f32, f32) {
    let tile = ctx.px(if badge.image.is_some() { 40.0 } else { 32.0 });
    (tile, ctx.px(10.0), ctx.px(11.0))
}

/// The size [`glass_badge`] draws `badge` at (physical pixels).
pub fn glass_badge_size(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    badge: &GlassBadge<'_>,
) -> (f32, f32) {
    let title = text.shape_line(badge.title, &ctx.ui_font(0.5).with_weight(Font::SEMIBOLD));
    let detail = badge.detail.map_or(0.0, |d| text.shape_line(d, &ctx.ui_font(-1.5)).width);
    let (tile, pad, gap) = glass_badge_parts(ctx, badge);
    let words = title.width.max(detail);
    ((pad + tile + gap + words + ctx.px(16.0)).round(), (pad * 2.0 + tile).round())
}

/// Draws `badge` in `area`. The frost is painted, not a backdrop blur: a blur would keep a copy
/// of the window for as long as the pane is up. Returns the pane.
pub fn glass_badge(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    area: Rect,
    badge: &GlassBadge<'_>,
) -> Rect {
    let px = |v: f32| ctx.px(v);
    let shown = |alpha: f32| badge.tint.with_alpha(alpha * badge.opacity);
    let title_font = ctx.ui_font(0.5).with_weight(Font::SEMIBOLD);
    let detail_font = ctx.ui_font(-1.5);
    let title = text.shape_line(badge.title, &title_font);
    let detail = badge.detail.map(|d| text.shape_line(d, &detail_font));
    let (tile, pad, gap) = glass_badge_parts(ctx, badge);
    let size = glass_badge_size(text, ctx, badge);
    let at = in_corner(area, size, badge.corner, px(badge.margin));
    let (x, y) = (at.x.round(), at.y.round());
    let pane = Rect::new(x, y, size.0, size.1);
    // Painted frost: a badge may stay up for hours (the dimmed screen).
    let look =
        crate::screen_widgets::GlassLook { tint: badge.tint, opacity: badge.opacity, night: false };
    crate::screen_widgets::glass_pane(layer, ctx, pane, look);
    let tile_at = Rect::new(x + pad, y + pad, tile, tile);
    if let Some(image) = badge.image {
        layer.images.push(render_api::ImageInstance {
            image: image.clone(),
            bounds: tile_at,
            opacity: badge.opacity.clamp(0.0, 1.0),
        });
    } else {
        layer.quad(Quad::filled(tile_at, shown(0.1)).rounded(px(radius::LG)));
    }
    if let Some(id) = icons.id(badge.icon).filter(|_| badge.image.is_none()) {
        let side = px(18.0);
        let inset = ((tile - side) / 2.0).round();
        let icon = Rect::new(tile_at.min_x() + inset, tile_at.min_y() + inset, side, side);
        ui_kit::draw_icon(layer, icons, id, icon, shown(0.9));
    }
    let words_x = tile_at.max_x() + gap;
    match &detail {
        Some(detail) => {
            let line = tile / 2.0;
            draw_text(
                layer,
                &title,
                words_x,
                optical_baseline(y + pad, line, &title_font),
                shown(0.92),
            );
            let below = y + pad + line;
            draw_text(
                layer,
                detail,
                words_x,
                optical_baseline(below, line, &detail_font),
                shown(0.58),
            );
        }
        None => {
            draw_text(
                layer,
                &title,
                words_x,
                optical_baseline(y + pad, tile, &title_font),
                shown(0.92),
            );
        }
    }
    pane
}

/// A changed file's badge's side, logical pixels.
pub const CHANGE_BADGE: f32 = 16.0;

/// A changed file's state as a small rounded square in its color (the theme's
/// `vcs.badge.*`) with its sign over it (`…` modified, `+` added, `−` deleted, `→` renamed,
/// `?` untracked, `!` conflicted), in `rect` (a square, on whole pixels).
pub fn change_badge(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    state: crate::changed_files::ChangeState,
) {
    let (fill, icon) = change_badge_look(state, ctx.palette);
    let r = ctx.px(radius::SM).min(rect.size.height / 2.0);
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(r));
    if let Some(id) = icons.id(icon) {
        let s = (rect.size.height * 0.72).round();
        let at = Rect::new(
            (rect.min_x() + (rect.size.width - s) / 2.0).round(),
            (rect.min_y() + (rect.size.height - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(sign_on(fill)));
    }
}

/// A state's badge color and the icon of its sign.
pub fn change_badge_look(
    state: crate::changed_files::ChangeState,
    p: &Palette,
) -> (Rgba, &'static str) {
    use crate::changed_files::ChangeState;
    let b = &p.vcs_badges;
    match state {
        ChangeState::Modified => (b.modified, "mark-modified"),
        ChangeState::Added => (b.added, "mark-added"),
        ChangeState::Deleted => (b.deleted, "mark-deleted"),
        ChangeState::Renamed => (b.renamed, "mark-renamed"),
        ChangeState::Untracked => (b.untracked, "mark-untracked"),
        ChangeState::Conflicted => (b.conflicted, "mark-conflicted"),
        ChangeState::Failed => (p.warning, "mark-conflicted"),
    }
}

/// The sign's color over `fill`: white on a dark fill, nearly black on a light one.
pub fn sign_on(fill: Rgba) -> Rgba {
    let luminance = 0.2126 * fill.r + 0.7152 * fill.g + 0.0722 * fill.b;
    if luminance > 0.6 { Rgba::rgb(0x1f2329) } else { Rgba::WHITE }
}
