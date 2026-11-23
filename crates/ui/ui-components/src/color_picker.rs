//! The color picker: a card next to the gutter swatch, with the saturation / brightness
//! square, the hue and alpha bands, the output format (HEX, RGBA, HSLA), the field with
//! the color text (editable) and the Cancel / Apply buttons.
//!
//! Gradients are small textures (generated once per hue / color), stretched on the GPU with
//! linear filtering: a few quads, not thousands.

use std::time::Duration;

use code_intelligence::{ColorFormat, ColorProvider};
use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, Layer, Quad, Scene};
use text_layout::TextSystem;
use ui_kit::{TextInput, TextInputStyle};

use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, radius};
use crate::palette::Palette;

/// The extension of the color picker's view: the colors module contributes it, the editor
/// opens it (a popover) when a color's swatch is clicked.
pub const PICKER_EXTENSION: &str = "view-color-picker";

/// What the picker is opened with: the swatch it opens next to, the code's color, its
/// format and where it is in the active file's text.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorPick {
    pub anchor: Rect,
    pub color: Rgba,
    pub format: ColorFormat,
    pub range: std::ops::Range<usize>,
}

/// Size of the saturation / brightness square's texture.
const SV_TEXELS: u32 = 64;

/// What was requested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerAction {
    /// Writes the color in the given format.
    Apply(Rgba, ColorFormat),
    Cancel,
    /// The eyedropper button: pick a color from the screen (the app starts it and calls
    /// [`ColorPicker::set_color`] with the result).
    Eyedropper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Sv,
    Hue,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Format(usize),
    Eyedropper,
    Cancel,
    Apply,
}

/// The formats' segmented control: its height (logical pixels).
const FORMATS_H: f32 = 28.0;

const FORMATS: [(ColorFormat, &str); 3] =
    [(ColorFormat::Hex, "HEX"), (ColorFormat::Rgb, "RGBA"), (ColorFormat::Hsl, "HSLA")];

/// See [`ColorPicker::enable_eyedropper`].
static EYEDROPPER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Default)]
pub struct ColorPicker {
    open: bool,
    anchor: Rect,
    viewport: Rect,
    scale: f32,
    /// Hue (0..360), saturation, value (HSV), alpha.
    hsva: [f32; 4],
    original: Rgba,
    format: Option<ColorFormat>,
    /// The initial format (a CSS name stays a name unless the format changes).
    initial_format: Option<ColorFormat>,
    input: TextInput,
    input_error: bool,
    drag: Option<Drag>,
    hover: Option<Part>,
    card: Rect,
    sv: Rect,
    hue: Rect,
    alpha: Rect,
    input_rect: Rect,
    /// The hex field has focus (clicked or typed into); otherwise no caret.
    input_focused: bool,
    parts: Vec<(Rect, Part)>,
    sv_image: Option<(u32, ImageData)>,
    hue_image: Option<ImageData>,
    alpha_image: Option<([u8; 3], ImageData)>,
    appear: Appear,
}

impl ColorPicker {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    /// Appearing (only while open: closed, it's gone at once and moves for nobody).
    pub fn is_animating(&self) -> bool {
        self.is_open() && self.appear.animating()
    }

    /// Opens the picker next to `anchor`, with the color and its format from the code.
    pub fn open(
        &mut self,
        anchor: Rect,
        color: Rgba,
        format: ColorFormat,
        colors: &dyn ColorProvider,
    ) {
        self.open = true;
        self.input_focused = false;
        self.anchor = anchor;
        self.original = color;
        self.hsva = to_hsva(color);
        self.format = Some(format);
        self.initial_format = Some(format);
        self.drag = None;
        self.hover = None;
        self.sync_input(colors);
        self.appear.start(Duration::from_millis(130));
    }

    /// Whether the platform can pick colors from the screen (set once by the app): every
    /// picker then shows the eyedropper button.
    pub fn enable_eyedropper(available: bool) {
        EYEDROPPER.store(available, std::sync::atomic::Ordering::Relaxed);
    }

    /// A color from elsewhere (the screen eyedropper): it becomes the current color.
    pub fn set_color(&mut self, color: Rgba, colors: &dyn ColorProvider) {
        self.hsva = to_hsva(color);
        self.sync_input(colors);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.drag = None;
        // The gradient textures are rebuilt on the next open.
        self.sv_image = None;
        self.alpha_image = None;
        self.hue_image = None;
    }

    pub fn contains(&self, p: Point) -> bool {
        self.open && self.card.contains(p)
    }

    pub fn color(&self) -> Rgba {
        from_hsva(self.hsva)
    }

    fn format(&self) -> ColorFormat {
        self.format.unwrap_or(ColorFormat::Hex)
    }

    fn sync_input(&mut self, colors: &dyn ColorProvider) {
        let text = colors.format(self.color(), self.format());
        self.input.set_text(&text);
        self.input.select_all();
        self.input_error = false;
    }

    fn apply(&self) -> PickerAction {
        PickerAction::Apply(self.color(), self.format())
    }

    /// Click: inside the card (drag, format, buttons) or outside it. Outside, what was
    /// chosen is kept (applied) — a stray click never loses it; with nothing changed it
    /// just closes. Only Cancel (or Esc) goes back to the original color.
    pub fn mouse_down(&mut self, p: Point, colors: &dyn ColorProvider) -> Option<PickerAction> {
        if !self.open {
            return None;
        }
        self.input_focused = self.input_rect.contains(p);
        if !self.card.contains(p) {
            let changed = self.color().to_rgba8() != self.original.to_rgba8()
                || self.format != self.initial_format;
            return Some(if changed { self.apply() } else { PickerAction::Cancel });
        }
        let grow = |r: Rect, by: f32| {
            Rect::new(
                r.min_x() - by,
                r.min_y() - by,
                r.size.width + 2.0 * by,
                r.size.height + 2.0 * by,
            )
        };
        let slop = (4.0 * self.scale).round();
        if self.sv.contains(p) {
            self.drag = Some(Drag::Sv);
        } else if grow(self.hue, slop).contains(p) {
            self.drag = Some(Drag::Hue);
        } else if grow(self.alpha, slop).contains(p) {
            self.drag = Some(Drag::Alpha);
        } else if let Some((_, part)) = self.parts.iter().find(|(r, _)| r.contains(p)).copied() {
            return match part {
                Part::Format(i) => {
                    self.format = Some(FORMATS[i].0);
                    self.sync_input(colors);
                    None
                }
                Part::Cancel => Some(PickerAction::Cancel),
                Part::Apply => Some(self.apply()),
                Part::Eyedropper => Some(PickerAction::Eyedropper),
            };
        }
        self.mouse_drag(p, colors);
        None
    }

    /// Drag in the square / bands. `true` if the color changed.
    pub fn mouse_drag(&mut self, p: Point, colors: &dyn ColorProvider) -> bool {
        let Some(drag) = self.drag else { return false };
        let fx = |r: Rect| ((p.x - r.min_x()) / r.size.width.max(1.0)).clamp(0.0, 1.0);
        let fy = |r: Rect| ((p.y - r.min_y()) / r.size.height.max(1.0)).clamp(0.0, 1.0);
        match drag {
            Drag::Sv => {
                self.hsva[1] = fx(self.sv);
                self.hsva[2] = 1.0 - fy(self.sv);
            }
            Drag::Hue => self.hsva[0] = fx(self.hue) * 360.0,
            Drag::Alpha => self.hsva[3] = fx(self.alpha),
        }
        // A CSS name can't hold any color: after a change, hex.
        if self.format == Some(ColorFormat::Named) {
            self.format = Some(ColorFormat::Hex);
        }
        self.sync_input(colors);
        true
    }

    /// A handle is being dragged (after a click in the square or on a band).
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn mouse_up(&mut self) -> bool {
        self.drag.take().is_some()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.parts.iter().find(|(r, _)| r.contains(p)).map(|(_, part)| *part);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// Something clickable is under the mouse (buttons, formats, square, bands).
    pub fn over_control(&self, p: Point) -> bool {
        self.parts.iter().any(|(r, _)| r.contains(p))
            || self.sv.contains(p)
            || self.hue.contains(p)
            || self.alpha.contains(p)
    }

    // ─── Text field ───

    pub fn type_text(&mut self, typed: &str, colors: &dyn ColorProvider) {
        self.input_focused = true;
        self.input.insert(typed);
        self.parse_input(colors);
    }

    pub fn backspace(&mut self, colors: &dyn ColorProvider) {
        self.input.backspace(false);
        self.parse_input(colors);
    }

    pub fn move_cursor(&mut self, right: bool, extend: bool) {
        if right {
            self.input.move_right(false, extend);
        } else {
            self.input.move_left(false, extend);
        }
    }

    /// The hex field (for clipboard / select-all routing).
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut TextInput {
        self.input_focused = true;
        &mut self.input
    }

    pub fn select_all(&mut self) {
        self.input.select_all();
    }

    fn parse_input(&mut self, colors: &dyn ColorProvider) {
        match colors.parse(self.input.text()) {
            Some(literal) => {
                // Hue is kept for grays (where it's undefined).
                let hue = self.hsva[0];
                self.hsva = to_hsva(literal.color);
                if self.hsva[1] == 0.0 {
                    self.hsva[0] = hue;
                }
                self.format = Some(literal.format);
                self.input_error = false;
            }
            None => self.input_error = true,
        }
    }

    /// Enter: apply (if the text is valid).
    pub fn confirm(&self) -> Option<PickerAction> {
        (!self.input_error).then(|| self.apply())
    }

    // ─── Painting ───

    pub fn paint(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        icons: &icons::Icons,
        palette: &Palette,
    ) {
        self.parts.clear();
        if !self.open {
            return;
        }
        let t = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(t);
        let px = |v: f32| ctx.px(v);
        let (w, h) = (px(284.0), px(356.0));
        let v = self.viewport;
        // Below the color's line (which stays visible), aligned with the swatch; above if it
        // doesn't fit below.
        let place = ds::PopoverPlace {
            side: ds::PopoverSide::Below,
            right: false,
            gap: px(ds::POPOVER_GAP),
            margin: px(8.0),
            bottom: px(8.0),
        };
        let (at, under) = ds::popover_rect(self.anchor, (w, h), v, place);
        // It comes toward its anchor as it appears.
        let lift = (1.0 - t) * px(6.0);
        let y = if under { at.min_y() + lift } else { at.min_y() - lift };
        let card = Rect::new(at.min_x().round(), y.round(), w, h);
        self.card = card;
        let layer = scene.push_layer(None);
        ds::surface(layer, &ctx, card, Elevation::Medium, radius::LG);

        let pad = px(14.0);
        let geo = Card { rect: card, pad, inner_w: w - 2.0 * pad, opacity: t };
        let y = self.paint_square(&geo, scene, &ctx);
        let (overlay, y) = self.paint_bands(y, &geo, scene, &ctx);
        self.paint_handles(overlay, &ctx);
        let y = self.paint_formats(y, &geo, (text, icons), overlay, &ctx);
        let field = self.paint_field(y, &geo, (text, icons), overlay, &ctx);
        self.paint_input(field, text, scene, &ctx);
        let y = field.max_y() + px(14.0);
        self.paint_buttons(y, &geo, (text, icons), scene, &ctx);
    }

    /// The saturation / brightness square (texture of the current hue), from the card's
    /// top; returns the y below it.
    fn paint_square(&mut self, geo: &Card, scene: &mut Scene, ctx: &Ctx<'_>) -> f32 {
        let card = geo.rect;
        let y = card.min_y() + geo.pad;
        self.sv = Rect::new(card.min_x() + geo.pad, y, geo.inner_w, ctx.px(150.0));
        let sv_image = self.sv_texture();
        let images = scene.push_layer(Some(card));
        images.images.push(render_api::ImageInstance {
            image: sv_image,
            bounds: self.sv,
            opacity: geo.opacity,
        });
        self.sv.max_y() + ctx.px(14.0)
    }

    /// The preview (old | new) and the hue and alpha bands at `y`, with the frames that
    /// round the corners; returns the layer over them and the y below.
    fn paint_bands<'s>(
        &mut self,
        y: f32,
        geo: &Card,
        scene: &'s mut Scene,
        ctx: &Ctx<'_>,
    ) -> (&'s mut Layer, f32) {
        let px = |v: f32| ctx.px(v);
        let card = geo.rect;
        let preview = Rect::new(card.min_x() + geo.pad, y, px(38.0), px(38.0));
        let bars_x = preview.max_x() + px(12.0);
        let bars_w = card.max_x() - geo.pad - bars_x;
        self.hue = Rect::new(bars_x, y + px(2.0), bars_w, px(12.0));
        self.alpha = Rect::new(bars_x, self.hue.max_y() + px(10.0), bars_w, px(12.0));
        let hue_image = self.hue_texture();
        let alpha_image = self.alpha_texture();
        // The checkerboard under what can be transparent: real squares (a stretched
        // texture smears into stripes on the long band).
        let squares = scene.push_layer(Some(card));
        let cell = (self.alpha.size.height / 2.0).round().max(1.0);
        checkerboard(squares, ctx, self.alpha, cell);
        checkerboard(squares, ctx, preview, cell);
        let bars = scene.push_layer(Some(card));
        for (r, image) in [(self.hue, &hue_image), (self.alpha, &alpha_image)] {
            bars.images.push(render_api::ImageInstance {
                image: image.clone(),
                bounds: r,
                opacity: geo.opacity,
            });
        }
        let half = Rect::new(
            preview.min_x(),
            preview.min_y(),
            preview.size.width / 2.0,
            preview.size.height,
        );
        let other = Rect::new(
            half.max_x(),
            preview.min_y(),
            preview.size.width - half.size.width,
            preview.size.height,
        );
        let overlay = scene.push_layer(Some(card));
        overlay.quad(Quad::filled(half, ctx.fade(self.original)));
        overlay.quad(Quad::filled(other, ctx.fade(self.color())));
        // Rounded corners: a frame in the card color over the edges.
        for (r, rad) in
            [(self.sv, radius::SM), (preview, radius::SM), (self.hue, 6.0), (self.alpha, 6.0)]
        {
            frame(overlay, ctx, r, px(rad));
        }
        (overlay, preview.max_y() + px(14.0))
    }

    /// The handles: on the square and on the hue and alpha bands.
    fn paint_handles(&self, overlay: &mut Layer, ctx: &Ctx<'_>) {
        let px = |v: f32| ctx.px(v);
        let color = self.color();
        let opaque = color.with_alpha(1.0);
        let sv_handle = Point::new(
            self.sv.min_x() + self.hsva[1] * self.sv.size.width,
            self.sv.min_y() + (1.0 - self.hsva[2]) * self.sv.size.height,
        );
        handle(overlay, ctx, sv_handle, px(14.0), opaque);
        let hue_color = from_hsva([self.hsva[0], 1.0, 1.0, 1.0]);
        let hx = self.hue.min_x() + self.hsva[0] / 360.0 * self.hue.size.width;
        handle(
            overlay,
            ctx,
            Point::new(hx, self.hue.min_y() + self.hue.size.height / 2.0),
            px(16.0),
            hue_color,
        );
        let ax = self.alpha.min_x() + self.hsva[3] * self.alpha.size.width;
        handle(
            overlay,
            ctx,
            Point::new(ax, self.alpha.min_y() + self.alpha.size.height / 2.0),
            px(16.0),
            color,
        );
    }

    /// The format at `y`: HEX | RGBA | HSLA; returns the y below it.
    fn paint_formats(
        &mut self,
        y: f32,
        geo: &Card,
        (text, icons): (&mut TextSystem, &icons::Icons),
        overlay: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> f32 {
        let px = |v: f32| ctx.px(v);
        let labels = FORMATS.map(|(_, label)| label);
        let spec = ds::SegmentedSpec {
            right: 0.0,
            mid: y + px(FORMATS_H) / 2.0,
            labels: &labels,
            selected: FORMATS
                .iter()
                .position(|(f, _)| Some(*f) == self.format)
                .unwrap_or(usize::MAX),
            hovered: (0..FORMATS.len()).find(|i| self.hover == Some(Part::Format(*i))),
        };
        let look = ds::SegmentedLook {
            height: FORMATS_H,
            place: ds::SegmentPlace::Fill { x: geo.rect.min_x() + geo.pad, width: geo.inner_w },
            ..Default::default()
        };
        let parts = &mut self.parts;
        ds::segmented_styled(overlay, text, icons, ctx, (spec, look), |i, r| {
            parts.push((r, Part::Format(i)));
        });
        y + px(FORMATS_H) + px(10.0)
    }

    /// The box of the field with the color text at `y` (and the eyedropper to its right);
    /// returns the field.
    fn paint_field(
        &mut self,
        y: f32,
        geo: &Card,
        (text, icons): (&mut TextSystem, &icons::Icons),
        overlay: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> Rect {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let eyedropper = EYEDROPPER.load(std::sync::atomic::Ordering::Relaxed);
        let dropper_w = if eyedropper { px(40.0) } else { 0.0 };
        let field = Rect::new(geo.rect.min_x() + geo.pad, y, geo.inner_w - dropper_w, px(32.0));
        if eyedropper {
            let b = Rect::new(
                field.max_x() + px(8.0),
                y,
                px(ds::button_size::SPACIOUS),
                px(ds::button_size::SPACIOUS),
            );
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Soft)
                .icon("pipette")
                .rest_tone(ds::Tone::Neutral)
                .hovered(self.hover == Some(Part::Eyedropper));
            ds::button(overlay, text, icons, ctx, b, spec);
            self.parts.push((b, Part::Eyedropper));
        }
        self.input_rect = field;
        let border = if self.input_error {
            p.danger
        } else if self.input_focused {
            p.accent.with_alpha(0.6)
        } else {
            p.card_border
        };
        overlay.quad(
            Quad::filled(field, ctx.fade(p.search_field_background))
                .rounded(px(radius::SM + 1.0))
                .bordered(ctx.line(), ctx.fade(border)),
        );
        field
    }

    /// The color text, on its own layer clipped to the `field`.
    fn paint_input(
        &mut self,
        field: Rect,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let p = ctx.palette;
        let style = TextInputStyle {
            font: ctx.code_font(-0.5),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: ctx.fade(p.text),
            placeholder: ctx.fade(p.text_muted),
            selection: ctx.fade(p.selection),
            caret: ctx.fade(p.caret),
            radius: 0.0,
            border_width: 0.0,
            padding: ctx.px(10.0),
            caret_width: ctx.px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        };
        let input_layer = scene.push_layer(Some(field));
        self.input.paint(input_layer, text, field, self.input_focused, "#rrggbb", &style);
    }

    /// The buttons at `y`: Cancel / Apply, on the right.
    fn paint_buttons(
        &mut self,
        y: f32,
        geo: &Card,
        (text, icons): (&mut TextSystem, &icons::Icons),
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(geo.rect));
        let button_h = px(30.0);
        let button_font = ds::button_font(ctx);
        let apply_label = i18n::t("color.apply");
        let cancel_label = i18n::t("dialog.cancel");
        let apply_w = (text.shape_line(&apply_label, &button_font).width + px(28.0)).max(px(76.0));
        let cancel_w =
            (text.shape_line(&cancel_label, &button_font).width + px(28.0)).max(px(76.0));
        let apply = Rect::new(geo.rect.max_x() - geo.pad - apply_w, y, apply_w, button_h);
        let cancel = Rect::new(apply.min_x() - px(8.0) - cancel_w, y, cancel_w, button_h);
        let apply_spec = ds::ButtonSpec::new(ds::ButtonKind::Primary)
            .label(&apply_label)
            .hovered(self.hover == Some(Part::Apply));
        let cancel_spec = ds::ButtonSpec::new(ds::ButtonKind::Secondary)
            .label(&cancel_label)
            .hovered(self.hover == Some(Part::Cancel));
        ds::button(layer, text, icons, ctx, apply, apply_spec);
        ds::button(layer, text, icons, ctx, cancel, cancel_spec);
        self.parts.push((apply, Part::Apply));
        self.parts.push((cancel, Part::Cancel));
    }

    fn sv_texture(&mut self) -> ImageData {
        let key = self.hsva[0].to_bits();
        if let Some((k, image)) = &self.sv_image
            && *k == key
        {
            return image.clone();
        }
        let n = SV_TEXELS;
        let mut pixels = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                let c = from_hsva([
                    self.hsva[0],
                    x as f32 / (n - 1) as f32,
                    1.0 - y as f32 / (n - 1) as f32,
                    1.0,
                ]);
                pixels.extend_from_slice(&c.to_rgba8());
            }
        }
        let image = ImageData::new(n, n, pixels);
        self.sv_image = Some((key, image.clone()));
        image
    }

    fn hue_texture(&mut self) -> ImageData {
        self.hue_image
            .get_or_insert_with(|| {
                let n = 128;
                let mut pixels = Vec::with_capacity(n * 4);
                for i in 0..n {
                    let c = from_hsva([i as f32 / (n - 1) as f32 * 360.0, 1.0, 1.0, 1.0]);
                    pixels.extend_from_slice(&c.to_rgba8());
                }
                ImageData::new(n as u32, 1, pixels)
            })
            .clone()
    }

    fn alpha_texture(&mut self) -> ImageData {
        let [r, g, b, _] = from_hsva([self.hsva[0], self.hsva[1], self.hsva[2], 1.0]).to_rgba8();
        if let Some((key, image)) = &self.alpha_image
            && *key == [r, g, b]
        {
            return image.clone();
        }
        let image = ImageData::new(2, 1, vec![r, g, b, 0, r, g, b, 255]);
        self.alpha_image = Some(([r, g, b], image.clone()));
        image
    }
}

/// The card's geometry, shared by the pieces of [`ColorPicker::paint`].
struct Card {
    rect: Rect,
    pad: f32,
    /// The width inside the padding.
    inner_w: f32,
    /// The appearance (0..1), for the images.
    opacity: f32,
}

/// The squares shown under what's transparent, whatever the theme (what everyone reads as
/// "see-through"): white and a light gray.
const CHECKER: (Rgba, Rgba) = (Rgba::rgb(0xffffff), Rgba::rgb(0xd6d6d6));

/// Light and dark squares of `cell` over `r` (behind a translucent color).
fn checkerboard(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect, cell: f32) {
    layer.quad(Quad::filled(r, ctx.fade(CHECKER.0)));
    let dark = ctx.fade(CHECKER.1);
    let (cols, rows) =
        ((r.size.width / cell).ceil() as usize, (r.size.height / cell).ceil() as usize);
    for row in 0..rows {
        for col in (row % 2..cols).step_by(2) {
            let x = r.min_x() + col as f32 * cell;
            let y = r.min_y() + row as f32 * cell;
            let w = cell.min(r.max_x() - x);
            let h = cell.min(r.max_y() - y);
            layer.quad(Quad::filled(Rect::new(x, y, w, h), dark));
        }
    }
}

/// The frame that rounds an image's corners (in the card color) and a thin border.
fn frame(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect, rad: f32) {
    let edge = ctx.px(3.0);
    let outer = Rect::new(
        r.min_x() - edge,
        r.min_y() - edge,
        r.size.width + 2.0 * edge,
        r.size.height + 2.0 * edge,
    );
    layer.quad(
        Quad::filled(outer, Rgba::TRANSPARENT)
            .rounded(rad + edge)
            .bordered(edge, ctx.fade(ctx.palette.popup_background)),
    );
    layer.quad(
        Quad::filled(r, Rgba::TRANSPARENT)
            .rounded(rad)
            .bordered(ctx.line(), ctx.fade(ctx.palette.text.with_alpha(0.10))),
    );
}

/// A round handle: white ring with a thin outline, filled with the color.
fn handle(layer: &mut Layer, ctx: &Ctx<'_>, center: Point, size: f32, color: Rgba) {
    let r = Rect::new((center.x - size / 2.0).round(), (center.y - size / 2.0).round(), size, size);
    let ring = ctx.px(2.5);
    let shadow = Rect::new(
        r.min_x() - ctx.line(),
        r.min_y() - ctx.line(),
        r.size.width + 2.0 * ctx.line(),
        r.size.height + 2.0 * ctx.line(),
    );
    layer.quad(
        Quad::filled(shadow, ctx.fade(Rgba::BLACK.with_alpha(0.25)))
            .rounded(shadow.size.width / 2.0),
    );
    layer.quad(
        Quad::filled(r, ctx.fade(color)).rounded(size / 2.0).bordered(ring, ctx.fade(Rgba::WHITE)),
    );
}

pub fn to_hsva(c: Rgba) -> [f32; 4] {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == c.r {
        60.0 * ((c.g - c.b) / d).rem_euclid(6.0)
    } else if max == c.g {
        60.0 * ((c.b - c.r) / d + 2.0)
    } else {
        60.0 * ((c.r - c.g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    [h, s, max, c.a]
}

pub fn from_hsva([h, s, v, a]: [f32; 4]) -> Rgba {
    let c = v * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Rgba::new(r + m, g + m, b + m, a)
}

/// An open color picker as a form's control: hit anywhere (outside it, it closes keeping a
/// changed color), its square and strips held while dragged; while open it takes the keys
/// whatever has the focus (which stays with the part under it, theirs again once it
/// closes): its field the text (⇧ ← → selecting, ⌫, ⌘A), ↵ applies, esc gives up. What it
/// asked for is written to `action`; `recolored` is set when the keys or the text changed
/// its color. `colors` reads and writes the color's formats.
pub struct ColorPart<'a> {
    pub picker: &'a mut ColorPicker,
    pub colors: &'a dyn ColorProvider,
    pub action: &'a mut Option<PickerAction>,
    pub recolored: &'a mut bool,
}

impl std::fmt::Debug for ColorPart<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorPart").finish_non_exhaustive()
    }
}

impl crate::control::Control for ColorPart<'_> {
    fn hit(&self, _p: Point) -> bool {
        self.picker.is_open()
    }

    fn press(&mut self, p: Point, _clicks: u32) -> crate::control::Press {
        *self.action = self.picker.mouse_down(p, self.colors);
        crate::control::Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.picker.mouse_drag(p, self.colors)
    }

    fn release(&mut self) -> bool {
        self.picker.mouse_up()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.picker.hover(p.unwrap_or(crate::control::NOWHERE))
    }

    fn cursor(&self, p: Point) -> Option<crate::ViewerCursor> {
        self.picker.over_control(p).then_some(crate::ViewerCursor::Pointer)
    }

    /// While open it takes the keys over its part (its field, ↵, esc).
    fn captures_keys(&self) -> bool {
        self.picker.is_open()
    }

    fn key(&mut self, key: crate::ViewerKey, reach: crate::KeyReach) -> bool {
        use crate::ViewerKey;
        match key {
            ViewerKey::Escape => *self.action = Some(PickerAction::Cancel),
            // Taken even with nothing to apply yet (a color half typed): never what's behind.
            ViewerKey::Enter => {
                if let Some(action) = self.picker.confirm() {
                    *self.action = Some(action);
                }
            }
            ViewerKey::Backspace => {
                self.picker.backspace(self.colors);
                *self.recolored = true;
            }
            ViewerKey::Left => self.picker.move_cursor(false, reach.extend),
            ViewerKey::Right => self.picker.move_cursor(true, reach.extend),
            _ => return false,
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.picker.type_text(typed, self.colors);
        *self.recolored = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        Some(self.picker.input().selected_text().to_owned()).filter(|s| !s.is_empty())
    }

    fn command(&mut self, id: &str) -> bool {
        if id != "select_all" {
            return false;
        }
        self.picker.select_all();
        true
    }

    fn animating(&self) -> bool {
        self.picker.is_animating()
    }
}

#[cfg(test)]
#[path = "../tests/unit/color_picker.rs"]
mod tests;
