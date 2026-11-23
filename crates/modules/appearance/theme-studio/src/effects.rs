//! The Effects category: the effects in use, a card each (folded to one line — its handle
//! for the order, its icon, its name, what it's set to in a few words, its switch, its menu:
//! Duplicate, Reset, Remove; opened, its options in sections: Look, Motion, Intensity,
//! Place), "+ Add effect" (a gallery: the presets, then every kind grouped by where it
//! shows — Edges, Background, Corners, Screen — each with a thumbnail moving under the
//! mouse, a line saying what it is and what it costs), "Use these with every theme", and
//! the decorations' own "Performance and accessibility" (animated / still / off, the frame
//! rate, when they stop, the system's reduce motion, the estimated cost).

use super::*;
use model::Global;
use panel::{Control, RowAt};
use theming::decorations::{
    Cost, Decorations, FieldKind, FieldValue, LayerKind, MAX_LAYERS, Merge, ResolveWith, presets,
};
use ui_components::decorations::{
    DecorationFrame, DecorationMode, Geometry, STATIC_TIME, WindowDecorations,
};
use ui_components::form::{self, FoldPart};

/// The sections of an effect's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Sec {
    Look,
    Motion,
    Intensity,
    Place,
}

impl Sec {
    pub(crate) const ALL: [Self; 4] = [Self::Look, Self::Motion, Self::Intensity, Self::Place];

    /// The section option `key` is in.
    pub(crate) fn of(key: &str) -> Self {
        match key {
            "speed" | "twinkle" => Self::Motion,
            "lights" | "density" | "glow" | "opacity" | "width" | "size" | "length" => {
                Self::Intensity
            }
            "edges" | "drop" | "zone" | "glass" => Self::Place,
            _ => Self::Look,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Look => "look",
            Self::Motion => "motion",
            Self::Intensity => "intensity",
            Self::Place => "place",
        }
    }

    /// Open until the user closes it (the look and the motion), or closed until opened.
    fn open_by_default(self) -> bool {
        matches!(self, Self::Look | Self::Motion)
    }
}

/// Where an effect shows (the gallery's groups, the first word of its summary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Edges,
    Background,
    Corners,
    Screen,
}

impl Place {
    pub(crate) const ALL: [Self; 4] = [Self::Edges, Self::Background, Self::Corners, Self::Screen];

    pub(crate) fn of(kind: LayerKind) -> Self {
        match kind {
            LayerKind::Garland | LayerKind::Neon => Self::Edges,
            LayerKind::Corners | LayerKind::HoloSigns => Self::Corners,
            LayerKind::Glow | LayerKind::Crt | LayerKind::Glitch => Self::Screen,
            _ => Self::Background,
        }
    }

    fn label(self) -> String {
        t(match self {
            Self::Edges => "studio.place_edges",
            Self::Background => "studio.place_background",
            Self::Corners => "studio.place_corners",
            Self::Screen => "studio.place_screen",
        })
    }
}

/// An effect's icon.
pub(crate) fn kind_icon(kind: LayerKind) -> &'static str {
    match kind {
        LayerKind::Garland => "lightbulb",
        LayerKind::Particles => "cloud",
        LayerKind::Corners => "tag",
        LayerKind::Glow => "sun",
        LayerKind::Matrix => "console",
        LayerKind::Starfield => "sparkles",
        LayerKind::Aurora => "layers",
        LayerKind::Fireflies => "pin",
        LayerKind::Bubbles => "globe",
        LayerKind::Fireworks => "flame",
        LayerKind::ShootingStars => "arrow-down",
        LayerKind::Waves => "rows",
        LayerKind::Neon => "square",
        LayerKind::Constellation => "graph",
        LayerKind::Synthwave => "sun",
        LayerKind::Lightning => "zap",
        LayerKind::Raindrops => "droplet",
        LayerKind::Clouds => "cloud",
        LayerKind::Fog => "cloud-fog",
        LayerKind::Butterflies => "butterfly",
        LayerKind::Balloons => "balloon",
        LayerKind::Hearts => "heart",
        LayerKind::Bats => "bat",
        LayerKind::LavaLamp => "lava-lamp",
        LayerKind::Crt => "tv",
        LayerKind::Glitch => "glitch",
        LayerKind::NeonRain => "cloud-rain",
        LayerKind::HoloSigns => "holo-sign",
    }
}

/// What an effect costs, as a tag: its height and the room on each side of its label
/// (logical pixels), and its fill's alpha of its tone.
const COST_H: f32 = 22.0;
const COST_PAD: f32 = 9.0;
const COST_FILL: f32 = 0.14;

/// What an effect costs, said.
fn cost_label(cost: Cost) -> String {
    t(match cost {
        Cost::Light => "studio.cost_light",
        Cost::Medium => "studio.cost_medium",
    })
}

/// What a row of the menu of an effect does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuItem {
    Duplicate,
    Reset,
    Remove,
}

/// The prefix of an effect's menu rows' ids: `effect:<row>:<effect's index>`.
const MENU_ID: &str = "effect:";

impl MenuItem {
    const ALL: [Self; 3] = [Self::Duplicate, Self::Reset, Self::Remove];

    fn label(self) -> String {
        t(match self {
            Self::Duplicate => "studio.deco_duplicate",
            Self::Reset => "studio.deco_reset",
            Self::Remove => "studio.deco_remove",
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Duplicate => "copy",
            Self::Reset => "rotate-ccw",
            Self::Remove => "trash",
        }
    }

    /// Its name in its row's id.
    fn id(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Reset => "reset",
            Self::Remove => "remove",
        }
    }

    /// Effect `i`'s menu, as the app shows menus: duplicate, reset, then remove apart (in
    /// the danger color).
    pub(crate) fn entries(i: usize) -> Vec<ViewMenuEntry> {
        let row = |item: Self| {
            ViewMenuEntry::item(format!("{MENU_ID}{}:{i}", item.id()), item.label())
                .with_icon(item.icon())
                .danger_if(item == Self::Remove)
        };
        vec![row(Self::Duplicate), row(Self::Reset), ViewMenuEntry::Separator, row(Self::Remove)]
    }

    /// The row and the effect a menu row's `id` names (`None`: not an effect's row).
    pub(crate) fn parse(id: &str) -> Option<(usize, Self)> {
        let (item, i) = id.strip_prefix(MENU_ID)?.split_once(':')?;
        let item = Self::ALL.into_iter().find(|m| m.id() == item)?;
        Some((i.parse().ok()?, item))
    }
}

/// An effect being dragged by its handle: which, and where the mouse is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drag {
    pub layer: usize,
    pub y: f32,
}

/// The decorations the preview draws now, and their painter (made again only when they
/// change).
#[derive(Debug)]
pub(crate) struct Live {
    pub decorations: Arc<Decorations>,
    pub painter: Box<dyn WindowDecorations>,
}

/// The draft's decorations as drawn (its settings' values, the layers switched on).
pub(crate) fn resolved(draft: &ThemeDraft) -> Decorations {
    let values: Vec<(String, theming::options::OptionValue)> =
        draft.options.iter().map(|o| (o.id.clone(), o.default.clone())).collect();
    draft.decorations.clone().resolve(ResolveWith {
        values: &values,
        palette: &[],
        dark: rows::dark(draft),
    })
}

/// A thumbnail in the gallery of effects: where it is, the key its painter is kept under
/// while the gallery is open, the effects it shows.
struct GalleryThumb {
    rect: Rect,
    key: String,
    layers: Vec<LayerSpec>,
}

impl Studio {
    /// Whether effect `i`'s card is open.
    pub(crate) fn layer_open(&self, i: usize) -> bool {
        self.open_layers.get(i).copied().unwrap_or(false)
    }

    /// Whether section `sec` of effect `i` is open.
    pub(crate) fn section_open(&self, i: usize, sec: Sec) -> bool {
        self.sections
            .iter()
            .find(|(l, s, _)| *l == i && *s == sec)
            .map_or(sec.open_by_default(), |(_, _, open)| *open)
    }

    pub(crate) fn toggle_section(&mut self, i: usize, sec: Sec) {
        let open = !self.section_open(i, sec);
        self.sections.retain(|(l, s, _)| !(*l == i && *s == sec));
        self.sections.push((i, sec, open));
    }

    /// Effect `i`'s card opened or closed.
    pub(crate) fn toggle_layer(&mut self, i: usize) {
        if self.open_layers.len() <= i {
            self.open_layers.resize(i + 1, false);
        }
        self.open_layers[i] = !self.open_layers[i];
    }

    /// The gallery closed, its thumbnails' painters let go.
    pub(crate) fn close_gallery(&mut self) {
        self.gallery_open = false;
        self.gallery_painters.clear();
        self.gallery_moving = false;
    }

    /// Every card and section opened (the tests, `open-all`).
    pub(crate) fn open_everything(&mut self) {
        let n = self.draft.as_ref().map_or(0, |d| d.decorations.layers.len());
        self.open_layers = vec![true; n];
        self.sections = (0..n).flat_map(|i| Sec::ALL.map(|s| (i, s, true))).collect();
    }

    /// The effects' panel; returns the `y` after it.
    pub(crate) fn paint_decorations(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let ds = col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let n = draft.decorations.layers.len();
        self.cards_at.clear();
        let mut y = self.header(pen, col, y, "active", "sparkles", &model::group_name("active"));
        // "Active effects (2)" and "+ Add effect".
        let font = ds.ui_font(-1.0);
        let label = if n == 0 {
            i18n::tr!("studio.active_count", n = 0)
        } else {
            i18n::tr!("studio.active_of", n = n as i64, max = MAX_LAYERS as i64)
        };
        let count = pen.text.shape_line(&label, &font);
        let bh = px(ds::BUTTON_H);
        draw_text(pen.layer, &count, col.x0, ds::optical_baseline(y, bh, &font), p.text_muted);
        let label = t("studio.add_effect");
        let bw = ds::primary_width(pen.text, &ds, &[&label]);
        let add = Rect::new(col.x0 + col.w - bw, y, bw, bh);
        let can_add = n < MAX_LAYERS;
        ds::primary_button(
            pen.layer,
            pen.text,
            &ds,
            add,
            &label,
            can_add,
            self.hover == Some(Hit::Gallery(true)),
        );
        if can_add {
            self.push_list_hit(add, Hit::Gallery(true));
        }
        y += bh + px(14.0);
        if n == 0 {
            y = self.paint_no_effects(pen, col, y);
        }
        for i in 0..n {
            if i > 0 {
                y += px(10.0);
            }
            y = self.paint_effect(pen, col, draft, base, y, i);
        }
        // Where a dragged effect would go.
        if let Some(drag) = self.drag
            && let Some(to) = self.drop_index(drag.y)
            && to != drag.layer
            && to != drag.layer + 1
        {
            let line_y = match self.cards_at.get(to) {
                Some((_, r)) => r.min_y() - px(5.0),
                None => self.cards_at.last().map_or(y, |(_, r)| r.max_y() + px(5.0)),
            };
            pen.layer.quad(
                Quad::filled(Rect::new(col.x0, line_y - px(1.0), col.w, px(2.0)), p.accent)
                    .rounded(px(1.0)),
            );
        }
        if n > 0 {
            y += px(16.0);
            y = self.paint_overlay_button(pen, col, y);
        }
        if ui_components::decorations::provider().is_some() {
            y = self.paint_performance(pen, col, draft, y);
        }
        y
    }

    /// No effects yet: a line saying so, and where to start.
    fn paint_no_effects(&mut self, pen: &mut LayerPainter<'_>, col: &Column<'_>, y: f32) -> f32 {
        let ds = col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let h = px(120.0);
        let r = Rect::new(col.x0, y, col.w, h);
        pen.layer.quad(
            Quad::filled(r, p.text.with_alpha(0.02))
                .rounded(px(radius::LG))
                .bordered(ds.line(), p.border.with_alpha(0.7)),
        );
        if let Some(id) = pen.icons.id("sparkles") {
            let side = px(22.0);
            let at = Rect::new(
                (r.min_x() + (r.size.width - side) / 2.0).round(),
                r.min_y() + px(22.0),
                side,
                side,
            );
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
        }
        let font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let title = pen.text.shape_line(&t("studio.deco_none_title"), &font);
        draw_text(
            pen.layer,
            &title,
            (r.min_x() + (r.size.width - title.width) / 2.0).round(),
            ds::optical_baseline(r.min_y() + px(52.0), px(20.0), &font),
            p.text,
        );
        let small = ds.ui_font(-1.5);
        let line = ds::ellipsize(pen.text, &t("studio.deco_empty"), &small, col.w - px(32.0));
        let line = pen.text.shape_line(&line, &small);
        draw_text(
            pen.layer,
            &line,
            (r.min_x() + (r.size.width - line.width) / 2.0).round(),
            ds::optical_baseline(r.min_y() + px(76.0), px(18.0), &small),
            p.text_muted,
        );
        r.max_y()
    }

    /// Effect `i`'s card: its header, and (opened) its options in sections; returns the `y`
    /// after it.
    fn paint_effect(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        top: f32,
        i: usize,
    ) -> f32 {
        let ds = col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(deco) = draft.decorations.layers.get(i) else { return top };
        let kind = deco.spec.kind();
        let card_at = pen.layer.quads.len();
        let open = self.layer_open(i);
        let hovered = match &self.hover {
            Some(Hit::Fold(h)) if *h == i => Some(FoldPart::Header),
            Some(Hit::Handle(h)) if *h == i => Some(FoldPart::Handle),
            Some(Hit::Toggle(h)) if *h == i => Some(FoldPart::Toggle),
            Some(Hit::Menu(h)) if *h == i => Some(FoldPart::Menu),
            _ => None,
        };
        let mut title = rows::kind_label(kind);
        if let Some(when) = &deco.when {
            let locale = i18n::locale();
            let name = draft
                .options
                .iter()
                .find(|o| o.id == when.option)
                .map_or_else(|| when.option.clone(), |o| o.label.get(&locale).to_owned());
            title = format!("{title} · {}", i18n::tr!("studio.deco_when", option = name.as_str()));
        }
        let summary = summary(deco);
        let area = form::fold_card_head(
            pen.layer,
            pen.text,
            &ds,
            pen.icons,
            form::FoldCardSpec {
                x: col.x0,
                y: top,
                width: col.w,
                icon: kind_icon(kind),
                tint: p.accent,
                title: &title,
                summary: &summary,
                expanded: open,
                enabled: deco.enabled,
                hovered,
            },
        );
        // The cost, after the title.
        self.push_list_hit(area.header, Hit::Fold(i));
        self.push_list_hit(area.handle, Hit::Handle(i));
        self.push_list_hit(area.toggle, Hit::Toggle(i));
        self.push_list_hit(area.menu, Hit::Menu(i));
        self.rows_at.push((SettingId::Layer(i), area.header));
        let mut y = area.header.max_y();
        if open {
            let inner = Column { ds, x0: col.x0 + px(12.0), w: col.w - px(24.0) };
            let icon_style = matches!(&deco.spec, LayerSpec::Corners(c) if c.style == theming::decorations::CornerStyle::Icon);
            for sec in Sec::ALL {
                let fields: Vec<(usize, &'static theming::decorations::Field)> = kind
                    .fields()
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| {
                        Sec::of(f.key) == sec && (f.kind != FieldKind::Text || icon_style)
                    })
                    .collect();
                if fields.is_empty() {
                    continue;
                }
                let sec_open = self.section_open(i, sec);
                let hovered = self.hover == Some(Hit::Section(i, sec));
                let r = form::fold_section(
                    pen,
                    (inner.x0, y, inner.w),
                    &t(&format!("studio.sec_{}", sec.id())),
                    sec_open,
                    hovered,
                );
                self.push_list_hit(r, Hit::Section(i, sec));
                y = r.max_y();
                if !sec_open {
                    continue;
                }
                for (field_at, field) in fields {
                    let row = Row::Field { layer: i, field: field_at };
                    let control = match field.kind {
                        FieldKind::Choice(ids) if ids.len() <= 3 => Control::Segmented(row),
                        FieldKind::Choice(_) => Control::Pills(row),
                        FieldKind::Amount(_) => Control::Slider(row),
                        FieldKind::Color | FieldKind::Colors { .. } => {
                            Control::Swatches(i, field.key)
                        }
                        FieldKind::Text => Control::Icons(Row::Icon(i)),
                    };
                    let label = if matches!(field.kind, FieldKind::Color | FieldKind::Colors { .. })
                    {
                        colors_label(draft, i, field.key)
                    } else {
                        row.label(draft)
                    };
                    y = self.paint_setting(
                        pen,
                        draft,
                        RowAt {
                            col: &inner,
                            y,
                            id: None,
                            label,
                            detail: row.detail(draft),
                            control,
                            reset: rows::differs(row, draft, base).then_some(Hit::ResetRow(row)),
                        },
                    );
                }
            }
            y += px(8.0);
        }
        let card = Rect::new(col.x0, top, col.w, y - top);
        let dragging = self.drag.is_some_and(|d| d.layer == i);
        pen.layer
            .quads
            .insert(card_at, form::fold_card_quad(&ds, card, hovered.is_some(), dragging));
        self.cards_at.push((i, card));
        y
    }

    /// The place an effect dragged to `y` goes (its index among the cards).
    pub(crate) fn drop_index(&self, y: f32) -> Option<usize> {
        if self.cards_at.is_empty() {
            return None;
        }
        Some(
            self.cards_at
                .iter()
                .position(|(_, r)| y < r.min_y() + r.size.height / 2.0)
                .unwrap_or(self.cards_at.len()),
        )
    }

    /// The effect dragged dropped where the mouse is: moved there (the open cards follow).
    pub(crate) fn finish_drag(&mut self) -> bool {
        let Some(drag) = self.drag.take() else { return false };
        let Some(to) = self.drop_index(drag.y) else { return true };
        let Some(draft) = &mut self.draft else { return true };
        let from = drag.layer;
        let n = draft.decorations.layers.len();
        if from >= n || to == from || to == from + 1 {
            return true;
        }
        let at = if to > from { to - 1 } else { to };
        let layer = draft.decorations.layers.remove(from);
        draft.decorations.layers.insert(at, layer);
        draft.decorations.merge = Merge::Replace;
        if self.open_layers.len() < n {
            self.open_layers.resize(n, false);
        }
        let open = self.open_layers.remove(from);
        self.open_layers.insert(at, open);
        self.sections.clear();
        self.preview();
        true
    }

    /// An effect's menu chose `item`.
    pub(crate) fn menu_item(&mut self, i: usize, item: MenuItem) {
        match item {
            MenuItem::Remove => {
                self.remove_layer(i);
                if i < self.open_layers.len() {
                    self.open_layers.remove(i);
                }
                self.sections.clear();
            }
            MenuItem::Duplicate => {
                let Some(draft) = &mut self.draft else { return };
                if draft.decorations.layers.len() >= MAX_LAYERS {
                    return;
                }
                let Some(copy) = draft.decorations.layers.get(i).cloned() else { return };
                draft.decorations.layers.insert(i + 1, copy);
                draft.decorations.merge = Merge::Replace;
                if self.open_layers.len() > i {
                    self.open_layers.insert(i + 1, true);
                }
                self.sections.clear();
                self.preview();
            }
            MenuItem::Reset => {
                let (Some(draft), Some(base)) = (&mut self.draft, &self.base_draft) else { return };
                let Some(layer) = draft.decorations.layers.get_mut(i) else { return };
                let kind = layer.spec.kind();
                *layer = match base.decorations.layers.get(i).filter(|b| b.spec.kind() == kind) {
                    Some(b) => b.clone(),
                    None => theming::decorations::DecorationLayer::plain(LayerSpec::new(kind)),
                };
                draft.decorations.merge = Merge::Replace;
                self.preview();
            }
        }
    }

    /// Effect `i` switched on or off.
    pub(crate) fn toggle_effect(&mut self, i: usize) {
        let Some(draft) = &mut self.draft else { return };
        let Some(layer) = draft.decorations.layers.get_mut(i) else { return };
        layer.enabled = !layer.enabled;
        draft.decorations.merge = Merge::Replace;
        self.preview();
    }

    /// "Performance and accessibility": the decorations' own settings (for every theme,
    /// saved at once), and what the effects in use cost.
    fn paint_performance(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let y = self.header(pen, col, y, "performance", "gpu", &model::group_name("performance"));
        let rows = [
            Some(Global::Mode),
            Some(Global::Fps),
            Some(Global::Idle),
            Some(Global::ReduceMotion),
            None,
        ];
        self.group(pen, col, y, rows.len(), |s, pen, n, y| match rows[n] {
            Some(g) => s.paint_setting(
                pen,
                draft,
                RowAt {
                    col,
                    y,
                    id: Some(SettingId::Global(g)),
                    label: t(&format!("studio.global_{}", g.key())),
                    detail: Some(t(&format!("studio.global_{}_detail", g.key()))),
                    control: Control::Global(g),
                    reset: None,
                },
            ),
            None => s.paint_cost(pen, col, draft, y),
        })
    }

    /// The estimated cost of the effects in use: a row with its tag.
    fn paint_cost(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let ds = col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let drawn = resolved(draft);
        let score: usize = drawn
            .layers
            .iter()
            .map(|l| match l.cost() {
                Cost::Light => 1,
                Cost::Medium => 2,
            })
            .sum();
        let (key, tone) = match score {
            0 => ("studio.cost_none", ds::Tone::Neutral),
            1..=3 => ("studio.cost_light", ds::Tone::Accent),
            4..=7 => ("studio.cost_medium", ds::Tone::Warning),
            _ => ("studio.cost_heavy", ds::Tone::Danger),
        };
        let h = px(form::ROW_H_DETAIL);
        let x = col.x0 + px(form::ROW_PAD);
        let label_font = ds.ui_font(-0.5);
        let small = ds.ui_font(-2.0);
        let shaped = pen.text.shape_line(&t("studio.cost"), &label_font);
        draw_text(
            pen.layer,
            &shaped,
            x,
            ds::optical_baseline(y + px(10.0), px(20.0), &label_font),
            p.text,
        );
        let detail = i18n::tr!("studio.cost_detail", n = drawn.layers.len() as i64);
        let detail = ds::ellipsize(pen.text, &detail, &small, col.w * 0.6);
        let shaped = pen.text.shape_line(&detail, &small);
        draw_text(
            pen.layer,
            &shaped,
            x,
            ds::optical_baseline(y + px(31.0), px(18.0), &small),
            p.text_muted,
        );
        let label = t(key);
        let font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let right = col.x0 + col.w - px(form::ROW_PAD) - px(form::RESET_W) - px(8.0);
        ds::Badge::new(&label)
            .color(tone.color(p))
            .fill_alpha(COST_FILL)
            .size(ds::BadgeSize::Custom(COST_H))
            .pad(COST_PAD)
            .font(&font)
            .paint_right(pen.layer, pen.text, &ds, right, y + h / 2.0);
        y + h
    }

    /// The gallery of effects (and presets), over the panel: open while choosing.
    pub(crate) fn paint_gallery(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        draft: &ThemeDraft,
    ) {
        if !self.gallery_open {
            return;
        }
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let area = self.list;
        let inset = px(16.0);
        let r = Rect::new(
            area.min_x() + inset,
            area.min_y() + px(8.0),
            area.size.width - 2.0 * inset,
            area.size.height - px(16.0),
        );
        if r.size.width <= 0.0 || r.size.height <= 0.0 {
            return;
        }
        let layer = scene.push_layer(Some(self.bounds));
        // The panel under it is covered to its edges: nothing of it shows around the card.
        layer.quad(Quad::filled(area, p.editor_background));
        ds::surface(layer, ds, r, ds::Elevation::High, radius::XL);
        // Its header: the title, ×.
        let pad = px(20.0);
        let title_font = ds.ui_font(2.0).with_weight(Font::SEMIBOLD);
        let title = text.shape_line(&t("studio.gallery_title"), &title_font);
        draw_text(
            layer,
            &title,
            r.min_x() + pad,
            ds::optical_baseline(r.min_y() + px(16.0), px(26.0), &title_font),
            p.text,
        );
        // Its line under the title, on two lines when narrow (the header grows to fit).
        let small = ds.ui_font(-1.5);
        let lines = ds::wrap(
            text,
            &t("studio.gallery_detail"),
            &small,
            r.size.width - 2.0 * pad - px(40.0),
            2,
        );
        for (i, line) in lines.iter().enumerate() {
            let sub = text.shape_line(line, &small);
            draw_text(
                layer,
                &sub,
                r.min_x() + pad,
                ds::optical_baseline(r.min_y() + px(42.0 + 18.0 * i as f32), px(18.0), &small),
                p.text_muted,
            );
        }
        let head = px(70.0 + 18.0 * (lines.len().max(1) - 1) as f32);
        let close = ds::clear_button(
            layer,
            icons,
            ds,
            r.max_x() - pad - px(9.0),
            r.min_y() + px(28.0),
            self.hover == Some(Hit::Gallery(false)),
        );
        self.hits.push((close, Hit::Gallery(false)));
        let body = Rect::new(
            r.min_x(),
            r.min_y() + head,
            r.size.width,
            (r.size.height - head - px(8.0)).max(0.0),
        );
        ds::divider(
            layer,
            ds,
            r.min_x() + ds.line(),
            r.max_x() - ds.line(),
            body.min_y() - px(4.0),
        );
        // The scrolled body: the presets, then the kinds by place.
        let layer = scene.push_layer(Some(body));
        let col = Column { ds: *ds, x0: body.min_x() + pad, w: body.size.width - 2.0 * pad };
        let mut y = body.min_y() + px(14.0) - self.gallery_scroll.round();
        let top = y;
        let dark = rows::dark(draft);
        let base_color = draft_color(draft, "base");
        let mut thumbs: Vec<GalleryThumb> = Vec::new();
        let mut cards: Vec<(Rect, Hit)> = Vec::new();
        let section = |layer: &mut Layer, text: &mut TextSystem, y: f32, title: &str| -> f32 {
            let font = ds.ui_font(-2.5).with_weight(Font::SEMIBOLD);
            let shaped = text.shape_line(&title.to_uppercase(), &font);
            draw_text(
                layer,
                &shaped,
                col.x0,
                ds::optical_baseline(y, px(18.0), &font),
                p.text_muted,
            );
            y + px(26.0)
        };
        // The presets.
        y = section(layer, text, y, &t("studio.gallery_presets"));
        let grid = form::Grid::new(col.x0, y, col.w, px(150.0), px(104.0), px(12.0));
        let current = Row::Preset.current(draft);
        for (n, id) in presets::ALL.iter().enumerate() {
            let cell = grid.cell(n);
            let hit = Hit::Choice(Row::Preset, n + 1);
            let name = t(&format!("decorations.preset_{}", id.replace('-', "_")));
            let thumb = form::gallery_card(
                layer,
                text,
                ds,
                form::GalleryCardSpec {
                    rect: cell,
                    thumb_h: 64.0,
                    title: &name,
                    detail: None,
                    badge: None,
                    selected: current == n + 1,
                    hovered: self.hover == Some(hit.clone()),
                },
            );
            thumbs.push(GalleryThumb {
                rect: thumb,
                key: format!("preset:{id}"),
                layers: presets::layers(id, dark).unwrap_or_default(),
            });
            cards.push((cell, hit));
        }
        y += grid.height(presets::ALL.len()) + px(22.0);
        // The kinds, by place.
        for place in Place::ALL {
            let kinds: Vec<(usize, LayerKind)> = LayerKind::ALL
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, k)| Place::of(*k) == place)
                .collect();
            if kinds.is_empty() {
                continue;
            }
            y = section(layer, text, y, &place.label());
            let grid = form::Grid::new(col.x0, y, col.w, px(170.0), px(134.0), px(12.0));
            for (n, (k, kind)) in kinds.iter().enumerate() {
                let cell = grid.cell(n);
                let hit = Hit::Choice(Row::Add, *k);
                let spec = LayerSpec::new(*kind);
                let detail = t(&format!("studio.kind_{}_detail", kind.id().replace('-', "_")));
                let cost = cost_label(spec.cost());
                let thumb = form::gallery_card(
                    layer,
                    text,
                    ds,
                    form::GalleryCardSpec {
                        rect: cell,
                        thumb_h: 72.0,
                        title: &rows::kind_label(*kind),
                        detail: Some(&detail),
                        badge: Some((
                            &cost,
                            if spec.cost() == Cost::Light {
                                ds::Tone::Neutral
                            } else {
                                ds::Tone::Warning
                            },
                        )),
                        selected: false,
                        hovered: self.hover == Some(hit.clone()),
                    },
                );
                let key = format!("kind:{}", kind.id());
                thumbs.push(GalleryThumb { rect: thumb, key, layers: vec![spec] });
                cards.push((cell, hit));
            }
            y += grid.height(kinds.len()) + px(22.0);
        }
        self.gallery_h = y - top;
        if self.gallery_to_end && self.gallery_h > 0.0 {
            self.gallery_to_end = false;
            self.gallery_scroll = (self.gallery_h - self.list.size.height * 0.6).max(0.0);
        }
        // The thumbnails: their background (the theme's), the effect drawn in it — moving
        // only under the mouse.
        let hovered_key = cards
            .iter()
            .zip(&thumbs)
            .find(|((_, hit), _)| self.hover.as_ref() == Some(hit))
            .map(|(_, thumb)| thumb.key.clone());
        self.gallery_moving = hovered_key.is_some();
        let time = self.anim_time();
        for thumb in &thumbs {
            let at = scene_layer_for(scene, thumb.rect, body);
            at.quad(Quad::filled(thumb.rect, base_color).rounded(px(radius::SM)));
            let moving = hovered_key.as_deref() == Some(thumb.key.as_str());
            let time = if moving { time } else { STATIC_TIME };
            self.paint_thumb_effect(at, thumb, dark, ds.scale, time, icons);
        }
        for (cell, hit) in cards {
            if let Some(cell) = cell.intersect(&body) {
                self.hits.push((cell, hit));
            }
        }
    }

    /// A thumbnail's effect, from a painter kept while the gallery is open.
    fn paint_thumb_effect(
        &mut self,
        layer: &mut Layer,
        thumb: &GalleryThumb,
        dark: bool,
        scale: f32,
        time: f32,
        icons: &icons::Icons,
    ) {
        let Some(provider) = ui_components::decorations::provider() else { return };
        let key = thumb.key.as_str();
        if !self.gallery_painters.iter().any(|(k, _)| k == key) {
            let decorations = Arc::new(Decorations { layers: thumb.layers.clone(), dark });
            let Some(painter) = provider.preview(&decorations) else { return };
            self.gallery_painters.push((key.to_owned(), painter));
        }
        let Some((_, painter)) = self.gallery_painters.iter_mut().find(|(k, _)| k == key) else {
            return;
        };
        let frame = DecorationFrame {
            geometry: Geometry {
                window: thumb.rect,
                scale,
                top_inset: 0.0,
                top_bar: 0.0,
                keep_clear: &[],
            },
            covered: &[],
            editor: &[],
            time,
            icons: Some(icons),
        };
        painter.paint(&frame, layer);
    }

    /// The time the effects move at (since the studio started animating; still when asked
    /// or idle).
    pub(crate) fn anim_time(&mut self) -> f32 {
        if !self.animating() {
            return STATIC_TIME;
        }
        let now = std::time::Instant::now();
        let start = *self.anim_start.get_or_insert(now);
        STATIC_TIME + now.saturating_duration_since(start).as_secs_f32()
    }

    /// Whether the preview's effects move now: asked to, something in them moves, and the
    /// user did something lately (they stop when idle, as the window's).
    pub(crate) fn animating(&self) -> bool {
        if self.preview_still || self.window_away {
            return false;
        }
        let idle = ui_components::decorations::provider()
            .map_or(Some(ui_components::decorations::IDLE_STOP), |p| p.idle());
        let recent = match (self.last_input, idle) {
            (_, None) => true,
            (Some(at), Some(idle)) => at.elapsed() < idle,
            (None, Some(_)) => false,
        };
        let moves =
            self.live.as_ref().is_some_and(|l| l.decorations.moves()) || self.gallery_moving;
        recent && moves
    }

    /// The preview's decorations for the draft (made again only when they change).
    pub(crate) fn sync_live(&mut self, draft: &ThemeDraft) {
        let wanted = resolved(draft);
        if self.live.as_ref().is_some_and(|l| *l.decorations == wanted) {
            return;
        }
        self.live = None;
        if wanted.is_empty() {
            return;
        }
        let Some(provider) = ui_components::decorations::provider() else { return };
        let decorations = Arc::new(wanted);
        if let Some(painter) = provider.preview(&decorations) {
            self.live = Some(Live { decorations, painter });
        }
    }

    /// "Use these with every theme" (and, once pressed, where they went); returns the `y`
    /// after it.
    pub(crate) fn paint_overlay_button(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        y: f32,
    ) -> f32 {
        let ds = &col.ds;
        let px = |v: f32| ds.px(v);
        let label = t("studio.deco_every_theme");
        let w = ds::primary_width(pen.text, ds, &[&label]).min(col.w);
        let r = Rect::new(col.x0, y, w, px(ds::BUTTON_H));
        ds::secondary_button(
            pen.layer,
            pen.text,
            ds,
            r,
            &label,
            true,
            self.hover == Some(Hit::Overlay),
        );
        self.push_list_hit(r, Hit::Overlay);
        let note_text = if self.overlay_saved {
            t("studio.deco_every_theme_done")
        } else {
            t("studio.deco_every_theme_detail")
        };
        note(pen, col, r.max_y() + px(8.0), &note_text)
    }

    /// A global setting's current choice and its choices (from the decorations' provider).
    pub(crate) fn global_choices(g: Global) -> (Vec<(i64, String)>, i64) {
        let provider = ui_components::decorations::provider();
        match g {
            Global::Mode => {
                let current = provider.as_ref().map(|p| p.mode()).unwrap_or_default();
                let choices = DecorationMode::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (i as i64, t(&format!("settings.decorations_{}", m.id()))))
                    .collect();
                let at = DecorationMode::ALL.iter().position(|m| *m == current).unwrap_or(0);
                (choices, at as i64)
            }
            Global::Fps => {
                let current =
                    provider.as_ref().map_or(ui_components::decorations::FPS, |p| p.fps());
                let choices = ui_components::decorations::FPS_CHOICES
                    .iter()
                    .map(|f| (i64::from(*f), f.to_string()))
                    .collect();
                (choices, i64::from(current))
            }
            Global::Idle => {
                let current = provider
                    .as_ref()
                    .map_or(Some(ui_components::decorations::IDLE_STOP), |p| p.idle());
                let secs = current.map_or(0, |d| d.as_secs() as i64);
                let choices = ui_components::decorations::IDLE_CHOICES
                    .iter()
                    .map(|s| (*s, ui_components::decorations::idle_label(*s)))
                    .collect();
                (choices, secs)
            }
            Global::ReduceMotion => {
                let on = provider.as_ref().is_none_or(|p| p.respects_system());
                (vec![(0, t("studio.off")), (1, t("studio.on"))], i64::from(on))
            }
        }
    }

    /// A global setting chosen: handed to the decorations' block (saved, every window
    /// follows).
    pub(crate) fn choose_global(&mut self, g: Global, value: i64) {
        use contracts::decorations::{DecorationPace, SetDecorationPace};
        let what = match g {
            Global::Mode => DecorationPace::Mode,
            Global::Fps => DecorationPace::Fps,
            Global::Idle => DecorationPace::Idle,
            Global::ReduceMotion => DecorationPace::FollowSystem,
        };
        self.pending =
            Some(ViewerAction::Ask(ui_components::Asked::new(SetDecorationPace { what, value })));
    }

    /// One color more in layer `i`'s list `key` (its last again), or one fewer.
    pub(crate) fn change_colors(&mut self, i: usize, key: &str, more: bool) {
        let Some(draft) = &mut self.draft else { return };
        let Some(deco) = draft.decorations.layers.get_mut(i) else { return };
        let Some(FieldValue::Colors(mut list)) = deco.spec.get(key) else { return };
        if more {
            list.push(list.last().copied().unwrap_or(Rgba::WHITE));
        } else {
            list.pop();
        }
        let len = list.len();
        if deco.spec.set(key, &FieldValue::Colors(list)).is_ok() {
            deco.bindings.retain(|b| b.key != key || b.index < len);
            draft.decorations.merge = Merge::Replace;
            self.preview();
        }
    }

    /// Layer `i` removed.
    pub(crate) fn remove_layer(&mut self, i: usize) {
        let Some(draft) = &mut self.draft else { return };
        if i < draft.decorations.layers.len() {
            draft.decorations.layers.remove(i);
            draft.decorations.merge = Merge::Replace;
            self.preview();
        }
    }

    /// The draft's decorations started from preset `id` (`none`: none); `false` if there's
    /// no such preset.
    pub(crate) fn use_preset(&mut self, id: &str) -> bool {
        let Some(draft) = &mut self.draft else { return false };
        let at = match id {
            "none" => 0,
            _ => match presets::ALL.iter().position(|p| *p == id) {
                Some(i) => i + 1,
                None => return false,
            },
        };
        Row::Preset.choose(draft, at);
        self.open_layers.clear();
        self.sections.clear();
        self.preview();
        true
    }
}

/// The layer a thumbnail's effect is drawn into: clipped to the thumbnail (and the
/// gallery's body).
fn scene_layer_for(scene: &mut Scene, thumb: Rect, body: Rect) -> &mut Layer {
    let clip = thumb.intersect(&body).unwrap_or(Rect::new(thumb.min_x(), thumb.min_y(), 0.0, 0.0));
    scene.push_layer(Some(clip))
}

/// What an effect is set to, in a few words: where it shows, then how dense and how fast.
pub(crate) fn summary(layer: &theming::decorations::DecorationLayer) -> String {
    let spec = &layer.spec;
    let place = match spec.zone() {
        theming::decorations::Zone::Edges => Place::of(spec.kind()).label(),
        theming::decorations::Zone::Panels => t("studio.v_panels"),
        theming::decorations::Zone::Everywhere => t("studio.v_everywhere"),
    };
    let mut parts = vec![place];
    for key in ["density", "lights", "speed", "twinkle"] {
        if let (Some(field), Some(FieldValue::Number(v))) = (spec.kind().field(key), spec.get(key))
            && let FieldKind::Amount(a) = field.kind
        {
            parts.push(rows::amount_words(key, v, a.max()).to_lowercase());
        }
        if parts.len() >= 3 {
            break;
        }
    }
    if !layer.enabled {
        parts.push(t("studio.deco_off").to_lowercase());
    }
    parts.join(" · ")
}

/// A layer's color option's label (and what it follows, if anything).
fn colors_label(draft: &ThemeDraft, i: usize, key: &str) -> String {
    use theming::decorations::BindTo;
    let label = t(&format!("studio.field_{key}"));
    let Some(deco) = draft.decorations.layers.get(i) else { return label };
    match deco.binding(key, 0) {
        Some(BindTo::Option(id)) => {
            let locale = i18n::locale();
            let name = draft
                .options
                .iter()
                .find(|o| o.id == *id)
                .map_or_else(|| id.clone(), |o| o.label.get(&locale).to_owned());
            format!("{label} · {}", i18n::tr!("studio.deco_follows", option = name.as_str()))
        }
        _ => label,
    }
}

/// A muted note under a part, wrapped to the column (two lines at most); returns the `y`
/// after it.
pub(crate) fn note(pen: &mut LayerPainter<'_>, col: &Column<'_>, y: f32, s: &str) -> f32 {
    let ds = &col.ds;
    let font = ds.ui_font(-1.5);
    let line_h = ds.px(18.0);
    let mut y = y;
    for line in ds::wrap(pen.text, s, &font, col.w, 2) {
        let shaped = pen.text.shape_line(&line, &font);
        draw_text(
            pen.layer,
            &shaped,
            col.x0,
            ds::optical_baseline(y, line_h, &font),
            ds.palette.text_muted,
        );
        y += line_h;
    }
    y
}
