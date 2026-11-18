//! The studio's rows of choices: the caret's, a decoration layer's options (from its kind's
//! schema), the theme's own settings, the presets, the kinds a layer may be added of, a
//! corner's icon — what each offers, which is chosen, and choosing.

use super::*;
use theming::backdrop::{BackdropKind, BackdropSpec, DEFAULT_BLUR, MAX_BLUR};
use theming::decorations::{
    FieldKind, FieldValue, LayerKind, MAX_LAYERS, Merge, presets, valid_name,
};
use theming::options::{OptionKind, OptionValue};

/// A row of choices in the configurator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    Caret(CaretRow),
    /// The backdrop under the windows over the editor.
    Backdrop(BackdropRow),
    /// The floating panels' glass (the menu drawer's).
    Floating(FloatingRow),
    /// Layer `layer`'s option `field` (its place among its kind's fields): a choice or an
    /// amount.
    Field {
        layer: usize,
        field: usize,
    },
    /// The theme's own setting `n` (its place).
    Option(usize),
    /// The decorations' presets (none first).
    Preset,
    /// A layer added, of one of the kinds.
    Add,
    /// Layer `n`'s icon (a corner's).
    Icon(usize),
}

/// None of the choices is lit (the presets when the layers are the theme's own; "Add").
pub(crate) const NONE_LIT: usize = usize::MAX;

/// The stops an amount offers: these shares of its maximum (a tenth apart).
const AMOUNTS: [f32; 11] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

/// The icons a corner may hold (the app's set has them).
pub(crate) const ICONS: [&str; 8] =
    ["bell", "lightbulb", "flame", "cloud", "globe", "palette", "pin", "tag"];

fn nearest(values: &[f32], v: f32) -> usize {
    (0..values.len())
        .min_by(|a, b| (values[*a] - v).abs().total_cmp(&(values[*b] - v).abs()))
        .unwrap_or(0)
}

pub(crate) fn percent(v: f32) -> String {
    i18n::tr!("studio.percent", n = (v * 100.0).round() as i64)
}

/// An amount said in words (its share of its maximum): how many for a density, how fast for
/// a speed, how far for a garland's drop, a percentage otherwise.
pub(crate) fn amount_words(key: &str, value: f32, max: f32) -> String {
    let share = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
    let step = if share <= 0.001 { 0 } else { ((share * 5.0).ceil() as usize).clamp(1, 5) };
    match key {
        "density" | "lights" => t(&format!("studio.amount_{step}")),
        "speed" | "twinkle" => t(&format!("studio.speed_{step}")),
        "drop" => i18n::tr!("studio.value_height", n = (value * 100.0).round() as i64),
        _ => percent(value),
    }
}

/// A choice's id as shown (`studio.v_<id>`).
pub(crate) fn value_label(id: &str) -> String {
    t(&format!("studio.v_{}", id.replace('-', "_")))
}

/// A kind's name.
pub(crate) fn kind_label(kind: LayerKind) -> String {
    t(&format!("studio.kind_{}", kind.id().replace('-', "_")))
}

/// Whether the draft is a dark theme (its editor's background).
pub(crate) fn dark(d: &ThemeDraft) -> bool {
    d.get("card").is_none_or(|c| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b < 0.5)
}

/// The theme's own setting `id`'s name.
fn option_name(d: &ThemeDraft, id: &str) -> String {
    let locale = i18n::locale();
    d.options
        .iter()
        .find(|o| o.id == id)
        .map_or_else(|| id.to_owned(), |o| o.label.get(&locale).to_owned())
}

impl Row {
    fn field(
        self,
        d: &ThemeDraft,
    ) -> Option<(&DecorationLayer, &'static theming::decorations::Field)> {
        let Self::Field { layer, field } = self else { return None };
        let l = d.decorations.layers.get(layer)?;
        Some((l, l.spec.kind().fields().get(field)?))
    }

    /// What it's labeled (a layer's option: what it follows, if anything).
    pub(crate) fn label(self, d: &ThemeDraft) -> String {
        match self {
            Self::Caret(r) => t(r.label()),
            Self::Backdrop(r) => t(&format!("studio.backdrop_{}", r.id())),
            Self::Floating(r) => t(&format!("studio.floating_{}", r.id())),
            Self::Field { .. } => {
                let Some((layer, field)) = self.field(d) else { return String::new() };
                let name = t(&format!("studio.field_{}", field.key));
                match layer.binding(field.key, 0) {
                    Some(theming::decorations::BindTo::Option(id)) => format!(
                        "{name} · {}",
                        i18n::tr!("studio.deco_follows", option = option_name(d, id).as_str())
                    ),
                    _ => name,
                }
            }
            Self::Option(n) => d
                .options
                .get(n)
                .map_or_else(String::new, |o| o.label.get(&i18n::locale()).to_owned()),
            Self::Preset => t("studio.deco_preset"),
            Self::Add => t("studio.deco_add"),
            Self::Icon(_) => t("studio.field_icon"),
        }
    }

    /// The line under its label saying what it does.
    pub(crate) fn detail(self, d: &ThemeDraft) -> Option<String> {
        match self {
            Self::Caret(r) => Some(t(&format!("studio.caret_{}_detail", r.id()))),
            Self::Backdrop(r) => Some(t(&format!("studio.backdrop_{}_detail", r.id()))),
            Self::Floating(r) => Some(t(&format!("studio.floating_{}_detail", r.id()))),
            Self::Field { .. } => {
                let (_, field) = self.field(d)?;
                Some(t(&format!("studio.field_{}_detail", field.key)))
            }
            Self::Icon(_) => Some(t("studio.field_icon_detail")),
            Self::Option(_) | Self::Preset | Self::Add => None,
        }
    }

    pub(crate) fn choices(self, d: &ThemeDraft) -> Vec<String> {
        match self {
            Self::Caret(r) => r.choices(),
            Self::Backdrop(r) => r.choices(),
            Self::Floating(r) => r.choices(),
            Self::Field { .. } => match self.field(d).map(|(_, f)| f.kind) {
                Some(FieldKind::Choice(ids)) => ids.iter().map(|id| value_label(id)).collect(),
                Some(FieldKind::Amount(a)) => {
                    AMOUNTS.iter().map(|s| percent(s * a.max())).collect()
                }
                _ => Vec::new(),
            },
            Self::Option(n) => match d.options.get(n).map(|o| (&o.kind, o)) {
                Some((OptionKind::Toggle, _)) => vec![t("studio.off"), t("studio.on")],
                Some((OptionKind::Choice(choices), _)) => {
                    let locale = i18n::locale();
                    choices.iter().map(|c| c.label.get(&locale).to_owned()).collect()
                }
                Some((OptionKind::Slider { min, max, .. }, o)) => o
                    .stops()
                    .iter()
                    .map(|s| {
                        if *min >= 0.0 && *max <= 1.0 {
                            percent(*s)
                        } else {
                            theming::decorations::number_text(*s)
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            },
            Self::Preset => std::iter::once(t("studio.preset_none"))
                .chain(
                    presets::ALL
                        .iter()
                        .map(|p| t(&format!("decorations.preset_{}", p.replace('-', "_")))),
                )
                .collect(),
            Self::Add => LayerKind::ALL.iter().map(|k| format!("+ {}", kind_label(*k))).collect(),
            Self::Icon(_) => ICONS.iter().map(|i| (*i).to_owned()).collect(),
        }
    }

    /// What its value means, in a few words (a slider's, said after it).
    pub(crate) fn value_text(self, d: &ThemeDraft) -> String {
        if let Self::Field { .. } = self
            && let Some((layer, field)) = self.field(d)
            && let (FieldKind::Amount(a), Some(FieldValue::Number(v))) =
                (field.kind, layer.spec.get(field.key))
        {
            return amount_words(field.key, v, a.max());
        }
        let choices = self.choices(d);
        choices.get(self.current(d)).cloned().unwrap_or_default()
    }

    /// How many stops it has (a slider's).
    pub(crate) fn stops(self, d: &ThemeDraft) -> usize {
        self.choices(d).len()
    }

    /// The choice the draft has ([`NONE_LIT`]: none).
    pub(crate) fn current(self, d: &ThemeDraft) -> usize {
        match self {
            Self::Caret(r) => r.current(&d.caret.look()),
            Self::Backdrop(r) => r.current(&d.backdrop),
            Self::Floating(r) => r.current(&d.floating),
            Self::Field { .. } => {
                let Some((layer, field)) = self.field(d) else { return 0 };
                match (field.kind, layer.spec.get(field.key)) {
                    (FieldKind::Choice(ids), Some(FieldValue::Text(v))) => {
                        ids.iter().position(|id| *id == v).unwrap_or(0)
                    }
                    (FieldKind::Amount(a), Some(FieldValue::Number(v))) => {
                        nearest(&AMOUNTS.map(|s| s * a.max()), v)
                    }
                    _ => 0,
                }
            }
            Self::Option(n) => {
                let Some(o) = d.options.get(n) else { return 0 };
                match (&o.kind, &o.default) {
                    (OptionKind::Toggle, OptionValue::Bool(b)) => usize::from(*b),
                    (OptionKind::Choice(choices), OptionValue::Choice(c)) => {
                        choices.iter().position(|x| x.id == *c).unwrap_or(0)
                    }
                    (OptionKind::Slider { .. }, OptionValue::Number(v)) => nearest(&o.stops(), *v),
                    _ => 0,
                }
            }
            Self::Preset => {
                let layers: Vec<&LayerSpec> =
                    d.decorations.layers.iter().map(|l| &l.spec).collect();
                if layers.is_empty() {
                    return 0;
                }
                presets::ALL
                    .iter()
                    .position(|p| {
                        presets::layers(p, dark(d)).is_some_and(|preset| {
                            preset.len() == layers.len()
                                && preset.iter().zip(&layers).all(|(a, b)| a == *b)
                        })
                    })
                    .map_or(NONE_LIT, |i| i + 1)
            }
            Self::Add => NONE_LIT,
            Self::Icon(n) => match d.decorations.layers.get(n).map(|l| &l.spec) {
                Some(LayerSpec::Corners(c)) => {
                    ICONS.iter().position(|i| *i == c.icon).unwrap_or(NONE_LIT)
                }
                _ => NONE_LIT,
            },
        }
    }

    /// Whether the draft says it itself (else it shows the default's).
    pub(crate) fn said(self, d: &ThemeDraft) -> bool {
        match self {
            Self::Caret(r) => row_set(&d.caret, r),
            Self::Backdrop(r) => r.said(&d.backdrop),
            Self::Floating(r) => r.said(&d.floating),
            _ => true,
        }
    }

    /// Its values are in order: a slider (else pills).
    pub(crate) fn is_slider(self, d: &ThemeDraft) -> bool {
        match self {
            Self::Caret(r) => r.is_slider(),
            Self::Backdrop(r) => r != BackdropRow::Style,
            Self::Floating(r) => r != FloatingRow::Style,
            Self::Field { .. } => {
                matches!(self.field(d).map(|(_, f)| f.kind), Some(FieldKind::Amount(_)))
            }
            Self::Option(n) => {
                matches!(d.options.get(n).map(|o| &o.kind), Some(OptionKind::Slider { .. }))
            }
            Self::Preset | Self::Add | Self::Icon(_) => false,
        }
    }

    /// Chooses its `i`th choice.
    pub(crate) fn choose(self, d: &mut ThemeDraft, i: usize) {
        match self {
            Self::Caret(r) => r.choose(&mut d.caret, i),
            Self::Backdrop(r) => r.choose(&mut d.backdrop, i),
            Self::Floating(r) => r.choose(&mut d.floating, i),
            Self::Field { layer, field } => {
                let Some(l) = d.decorations.layers.get_mut(layer) else { return };
                let Some(f) = l.spec.kind().fields().get(field) else { return };
                let value = match f.kind {
                    FieldKind::Choice(ids) => match ids.get(i) {
                        Some(id) => FieldValue::Text((*id).to_owned()),
                        None => return,
                    },
                    FieldKind::Amount(a) => match AMOUNTS.get(i) {
                        Some(s) => FieldValue::Number(s * a.max()),
                        None => return,
                    },
                    _ => return,
                };
                if l.spec.set(f.key, &value).is_ok() {
                    l.unbind(f.key);
                    d.decorations.merge = Merge::Replace;
                }
            }
            Self::Option(n) => {
                let Some(o) = d.options.get_mut(n) else { return };
                let value = match &o.kind {
                    OptionKind::Toggle => OptionValue::Bool(i == 1),
                    OptionKind::Choice(choices) => match choices.get(i) {
                        Some(c) => OptionValue::Choice(c.id.clone()),
                        None => return,
                    },
                    OptionKind::Slider { .. } => match o.stops().get(i) {
                        Some(v) => OptionValue::Number(*v),
                        None => return,
                    },
                    OptionKind::Color { .. } => return,
                };
                o.default = value;
            }
            Self::Preset => {
                let layers = match i {
                    0 => Vec::new(),
                    _ => match presets::ALL.get(i - 1).and_then(|p| presets::layers(p, dark(d))) {
                        Some(layers) => layers,
                        None => return,
                    },
                };
                d.decorations = theming::decorations::DecorationsSpec::of(layers);
            }
            Self::Add => {
                if let Some(kind) = LayerKind::ALL.get(i)
                    && d.decorations.layers.len() < MAX_LAYERS
                {
                    d.decorations.layers.push(DecorationLayer::plain(LayerSpec::new(*kind)));
                    d.decorations.merge = Merge::Replace;
                }
            }
            Self::Icon(n) => {
                if let Some(icon) = ICONS.get(i).filter(|i| valid_name(i))
                    && let Some(l) = d.decorations.layers.get_mut(n)
                    && l.spec.set("icon", &FieldValue::Text((*icon).to_owned())).is_ok()
                {
                    d.decorations.merge = Merge::Replace;
                }
            }
        }
    }
}

/// Row `row` put back in `d` as `base` has it: a caret's row, a theme setting's default, a
/// layer's option (as its layer in `base` has it when of the same kind, else its kind's
/// default).
pub(crate) fn reset(row: Row, d: &mut ThemeDraft, base: &ThemeDraft) {
    match row {
        Row::Caret(r) => {
            let b = &base.caret;
            let c = &mut d.caret;
            match r {
                CaretRow::Shape => c.shape = b.shape,
                CaretRow::Width => c.width = b.width,
                CaretRow::Blink => c.blink = b.blink,
                CaretRow::Motion => c.motion = b.motion,
                CaretRow::Glow => c.glow = b.glow,
                CaretRow::Trail => c.trail_ms = b.trail_ms,
            }
        }
        Row::Backdrop(r) => {
            let (b, c) = (&base.backdrop, &mut d.backdrop);
            match r {
                BackdropRow::Style => c.style = b.style,
                BackdropRow::Opacity => c.opacity = b.opacity,
                BackdropRow::Blur => c.blur = b.blur,
            }
        }
        Row::Floating(r) => {
            let (b, c) = (&base.floating, &mut d.floating);
            match r {
                FloatingRow::Style => c.style = b.style,
                FloatingRow::Opacity => c.opacity = b.opacity,
                FloatingRow::Blur => c.blur = b.blur,
            }
        }
        Row::Option(n) => {
            if let (Some(o), Some(b)) = (d.options.get_mut(n), base.options.get(n)) {
                o.default = b.default.clone();
            }
        }
        Row::Field { layer, field } => {
            let Some(l) = d.decorations.layers.get_mut(layer) else { return };
            let kind = l.spec.kind();
            let Some(f) = kind.fields().get(field) else { return };
            let from = base.decorations.layers.get(layer).filter(|b| b.spec.kind() == kind);
            let value = match from {
                Some(b) => b.spec.get(f.key),
                None => LayerSpec::new(kind).get(f.key),
            };
            if let Some(v) = value
                && l.spec.set(f.key, &v).is_ok()
            {
                l.unbind(f.key);
                if let Some(b) = from {
                    l.bindings.extend(b.bindings.iter().filter(|x| x.key == f.key).cloned());
                }
                d.decorations.merge = Merge::Replace;
            }
        }
        Row::Preset | Row::Add | Row::Icon(_) => {}
    }
}

/// Whether row `row` differs in `d` from `base` (its "↺").
pub(crate) fn differs(row: Row, d: &ThemeDraft, base: &ThemeDraft) -> bool {
    match row {
        Row::Caret(r) => {
            let (a, b) = (Row::Caret(r), Row::Caret(r));
            a.current(d) != b.current(base) || a.said(d) != b.said(base)
        }
        Row::Backdrop(r) => {
            r.current(&d.backdrop) != r.current(&base.backdrop)
                || r.said(&d.backdrop) != r.said(&base.backdrop)
        }
        Row::Floating(r) => {
            r.current(&d.floating) != r.current(&base.floating)
                || r.said(&d.floating) != r.said(&base.floating)
        }
        Row::Option(n) => {
            d.options.get(n).map(|o| &o.default) != base.options.get(n).map(|o| &o.default)
        }
        Row::Field { layer, field } => {
            let Some(l) = d.decorations.layers.get(layer) else { return false };
            let kind = l.spec.kind();
            let Some(f) = kind.fields().get(field) else { return false };
            // A layer the starting theme doesn't have is a change as a whole (its options
            // have nothing to go back to).
            let Some(from) = base.decorations.layers.get(layer).filter(|b| b.spec.kind() == kind)
            else {
                return false;
            };
            l.spec.get(f.key) != from.spec.get(f.key)
        }
        Row::Preset | Row::Add | Row::Icon(_) => false,
    }
}

/// Whether the theme says caret `row` itself (else it shows the default's).
fn row_set(spec: &CaretSpec, row: CaretRow) -> bool {
    match row {
        CaretRow::Shape => spec.shape.is_some(),
        CaretRow::Width => spec.width.is_some(),
        CaretRow::Blink => spec.blink.is_some(),
        CaretRow::Motion => spec.motion.is_some(),
        CaretRow::Glow => spec.glow.is_some(),
        CaretRow::Trail => spec.trail_ms.is_some(),
    }
}

/// A row of the caret's choices in the configurator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CaretRow {
    Shape,
    Width,
    Blink,
    Motion,
    Glow,
    Trail,
}

const SHAPES: [CaretShape; 3] = [CaretShape::Bar, CaretShape::Block, CaretShape::Underline];
const BLINKS: [Blink; 3] = [Blink::Off, Blink::Classic, Blink::Smooth];
const MOTIONS: [Motion; 2] = [Motion::Jump, Motion::Glide];
const WIDTHS: [f32; 4] = [1.0, 2.0, 3.0, 4.0];
const GLOWS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const TRAILS: [u32; 5] = [0, 120, 220, 400, 700];

impl CaretRow {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 6] =
        [Self::Shape, Self::Width, Self::Blink, Self::Motion, Self::Glow, Self::Trail];

    /// Its id (its description's key: `studio.caret_<id>_detail`).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::Width => "width",
            Self::Blink => "blink",
            Self::Motion => "motion",
            Self::Glow => "glow",
            Self::Trail => "trail",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Shape => "settings.caret_shape",
            Self::Width => "settings.caret_width",
            Self::Blink => "settings.caret_blink",
            Self::Motion => "settings.caret_motion",
            Self::Glow => "settings.caret_glow",
            Self::Trail => "settings.caret_trail",
        }
    }

    fn choices(self) -> Vec<String> {
        match self {
            Self::Shape => {
                ["settings.caret_bar", "settings.caret_block", "settings.caret_underline"]
                    .map(t)
                    .into()
            }
            Self::Width => WIDTHS.iter().map(|w| format!("{w} px")).collect(),
            Self::Blink => [
                "settings.caret_blink_none",
                "settings.caret_blink_classic",
                "settings.caret_blink_smooth",
            ]
            .map(t)
            .into(),
            Self::Motion => ["settings.caret_jump", "settings.caret_glide"].map(t).into(),
            Self::Glow => GLOWS.iter().map(|g| percent(*g)).collect(),
            Self::Trail => TRAILS
                .iter()
                .map(|ms| {
                    if *ms == 0 {
                        t("settings.caret_none")
                    } else {
                        i18n::tr!("settings.caret_ms", ms = *ms)
                    }
                })
                .collect(),
        }
    }

    /// The choice the look has (the nearest, for values set by hand).
    fn current(self, look: &CaretLook) -> usize {
        match self {
            Self::Shape => SHAPES.iter().position(|s| *s == look.shape).unwrap_or(0),
            Self::Width => nearest(&WIDTHS, look.width),
            Self::Blink => BLINKS.iter().position(|b| *b == look.blink).unwrap_or(0),
            Self::Motion => MOTIONS.iter().position(|m| *m == look.motion).unwrap_or(0),
            Self::Glow => nearest(&GLOWS, look.glow),
            Self::Trail => nearest(&TRAILS.map(|t| t as f32), look.trail_ms as f32),
        }
    }

    /// Its values are in order: a slider (else pills).
    fn is_slider(self) -> bool {
        matches!(self, Self::Width | Self::Glow | Self::Trail)
    }

    /// Chooses its `i`th choice in the theme's caret.
    fn choose(self, spec: &mut CaretSpec, i: usize) {
        match self {
            Self::Shape => spec.shape = SHAPES.get(i).copied(),
            Self::Width => spec.width = WIDTHS.get(i).copied(),
            Self::Blink => spec.blink = BLINKS.get(i).copied(),
            Self::Motion => spec.motion = MOTIONS.get(i).copied(),
            Self::Glow => spec.glow = GLOWS.get(i).copied(),
            Self::Trail => spec.trail_ms = TRAILS.get(i).copied(),
        }
    }
}

/// A row of the backdrop's choices: how it shows the window behind, how much of its color,
/// how far it blurs (its color is a swatch: [`theming::studio::BACKDROP_COLOR`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BackdropRow {
    Style,
    Opacity,
    Blur,
}

const BACKDROP_KINDS: [BackdropKind; 2] = [BackdropKind::Simple, BackdropKind::Glass];
/// The color's stops, after "automatic" (each window's own).
const BACKDROP_OPACITIES: [f32; 13] =
    [0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6];

impl BackdropRow {
    pub(crate) const ALL: [Self; 3] = [Self::Style, Self::Opacity, Self::Blur];

    /// Its id (its label's key: `studio.backdrop_<id>`).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Opacity => "opacity",
            Self::Blur => "blur",
        }
    }

    fn choices(self) -> Vec<String> {
        match self {
            Self::Style => ["settings.backdrop_simple", "settings.backdrop_glass"].map(t).into(),
            Self::Opacity => std::iter::once(t("studio.backdrop_auto"))
                .chain(BACKDROP_OPACITIES.iter().map(|o| percent(*o)))
                .collect(),
            Self::Blur => (0..=MAX_BLUR as i64)
                .map(|px| {
                    if px == 0 {
                        t("settings.backdrop_no_blur")
                    } else {
                        i18n::tr!("settings.backdrop_px", n = px)
                    }
                })
                .collect(),
        }
    }

    fn current(self, spec: &BackdropSpec) -> usize {
        match self {
            Self::Style => BACKDROP_KINDS.iter().position(|k| Some(*k) == spec.style).unwrap_or(0),
            Self::Opacity => spec.opacity.map_or(0, |o| nearest(&BACKDROP_OPACITIES, o) + 1),
            Self::Blur => spec.blur.unwrap_or(DEFAULT_BLUR).round().clamp(0.0, MAX_BLUR) as usize,
        }
    }

    fn said(self, spec: &BackdropSpec) -> bool {
        match self {
            Self::Style => spec.style.is_some(),
            Self::Opacity => spec.opacity.is_some(),
            Self::Blur => spec.blur.is_some(),
        }
    }

    fn choose(self, spec: &mut BackdropSpec, i: usize) {
        match self {
            Self::Style => spec.style = BACKDROP_KINDS.get(i).copied(),
            Self::Opacity => {
                spec.opacity = i.checked_sub(1).and_then(|i| BACKDROP_OPACITIES.get(i).copied());
            }
            Self::Blur => spec.blur = Some((i as f32).min(MAX_BLUR)),
        }
    }
}

/// A row of the floating panels' glass (`[floating]`: the menu drawer's): opaque or glass,
/// the panel's opacity, how far it blurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FloatingRow {
    Style,
    Opacity,
    Blur,
}

/// The panel's opacity stops.
const FLOATING_OPACITIES: [f32; 9] = [0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0];

impl FloatingRow {
    pub(crate) const ALL: [Self; 3] = [Self::Style, Self::Opacity, Self::Blur];

    /// Its id (its label's key: `studio.floating_<id>`).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Opacity => "opacity",
            Self::Blur => "blur",
        }
    }

    fn choices(self) -> Vec<String> {
        match self {
            Self::Opacity => FLOATING_OPACITIES.iter().map(|o| percent(*o)).collect(),
            Self::Style => BackdropRow::Style.choices(),
            Self::Blur => BackdropRow::Blur.choices(),
        }
    }

    fn current(self, spec: &BackdropSpec) -> usize {
        match self {
            Self::Style => BackdropRow::Style.current(spec),
            Self::Opacity => nearest(
                &FLOATING_OPACITIES,
                spec.opacity.unwrap_or(theming::backdrop::PANEL_OPACITY),
            ),
            Self::Blur => BackdropRow::Blur.current(spec),
        }
    }

    fn said(self, spec: &BackdropSpec) -> bool {
        match self {
            Self::Style => spec.style.is_some(),
            Self::Opacity => spec.opacity.is_some(),
            Self::Blur => spec.blur.is_some(),
        }
    }

    fn choose(self, spec: &mut BackdropSpec, i: usize) {
        match self {
            Self::Style => BackdropRow::Style.choose(spec, i),
            Self::Opacity => spec.opacity = FLOATING_OPACITIES.get(i).copied(),
            Self::Blur => BackdropRow::Blur.choose(spec, i),
        }
    }
}
