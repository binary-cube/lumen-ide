//! A layer's options described: the kind of value each takes and its limit, every kind's
//! list, a value given and why it can't be taken, numbers and colors as a theme writes
//! them.

use kernel::Rgba;

use super::{
    CornerStyle, GarlandEdges, GlowStyle, Items, LayerKind, MAX_LIGHT_COLORS, MAX_ORNAMENT_COLORS,
    ParticleShape, Strand, Zone, effect,
};

/// The kind of value an option of a layer takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// One of these ids.
    Choice(&'static [&'static str]),
    /// A number from 0 to its maximum.
    Amount(Amount),
    Color,
    /// A list of colors, from `min` to `max` of them.
    Colors {
        min: usize,
        max: usize,
    },
    /// A name (an icon's).
    Text,
}

/// An amount's maximum, kept exactly (`f32` isn't `Eq`): in hundredths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(pub u16);

impl Amount {
    pub fn max(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

/// An option of a layer: its key (as written in a theme) and the value it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub kind: FieldKind,
}

pub(super) const fn field(key: &'static str, kind: FieldKind) -> Field {
    Field { key, kind }
}

const UNIT: FieldKind = FieldKind::Amount(Amount(100));

const GARLAND_FIELDS: &[Field] = &[
    field("strand", FieldKind::Choice(Strand::IDS)),
    field("items", FieldKind::Choice(Items::IDS)),
    field("edges", FieldKind::Choice(GarlandEdges::IDS)),
    field("lights", UNIT),
    field("twinkle", UNIT),
    field("glow", UNIT),
    field("colors", FieldKind::Colors { min: 1, max: MAX_LIGHT_COLORS }),
    field("ornaments", FieldKind::Colors { min: 0, max: MAX_ORNAMENT_COLORS }),
    field("wire", FieldKind::Color),
    field("drop", FieldKind::Amount(Amount(80))),
    field("opacity", UNIT),
];

const PARTICLE_FIELDS: &[Field] = &[
    field("shape", FieldKind::Choice(ParticleShape::IDS)),
    field("density", UNIT),
    field("speed", UNIT),
    field("colors", FieldKind::Colors { min: 1, max: MAX_LIGHT_COLORS }),
    field("opacity", FieldKind::Amount(Amount(50))),
    field("zone", FieldKind::Choice(Zone::IDS)),
    field("glass", UNIT),
];

const CORNER_FIELDS: &[Field] = &[
    field("style", FieldKind::Choice(CornerStyle::IDS)),
    field("icon", FieldKind::Text),
    field("colors", FieldKind::Colors { min: 1, max: 2 }),
    field("opacity", UNIT),
];

const GLOW_FIELDS: &[Field] = &[
    field("style", FieldKind::Choice(GlowStyle::IDS)),
    field("color", FieldKind::Color),
    field("opacity", FieldKind::Amount(Amount(60))),
    field("width", UNIT),
];

impl LayerKind {
    /// Its options, in the order they're written and shown.
    pub fn fields(self) -> &'static [Field] {
        match self {
            Self::Garland => GARLAND_FIELDS,
            Self::Particles => PARTICLE_FIELDS,
            Self::Corners => CORNER_FIELDS,
            Self::Glow => GLOW_FIELDS,
            kind => effect::fields(kind),
        }
    }

    /// Its option `key`.
    pub fn field(self, key: &str) -> Option<&'static Field> {
        self.fields().iter().find(|f| f.key == key)
    }
}

/// A value of a layer's option.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Number(f32),
    Text(String),
    Color(Rgba),
    Colors(Vec<Rgba>),
}

/// Why a layer's option can't take a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldError {
    /// The kind has no such option.
    Unknown,
    /// Not a value it takes (why).
    Invalid(&'static str),
}

/// A number written as a theme writes it (two decimals at most, no trailing zeros).
pub fn number_text(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    format!("{rounded}")
}

/// A color as a theme writes it (`#rrggbb`, `#rrggbbaa` when translucent).
pub fn hex(c: Rgba) -> String {
    let [r, g, b, a] = c.to_rgba8();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

pub(super) fn amount(value: &FieldValue, max: f32) -> Result<f32, FieldError> {
    match value {
        FieldValue::Number(v) if v.is_finite() && (0.0..=max + 1e-6).contains(v) => Ok(v.min(max)),
        FieldValue::Number(_) => {
            Err(FieldError::Invalid(if max == 1.0 { "from 0 to 1" } else { "out of range" }))
        }
        _ => Err(FieldError::Invalid("a number")),
    }
}

pub(super) fn choice<T: Copy>(
    value: &FieldValue,
    parse: fn(&str) -> Option<T>,
) -> Result<T, FieldError> {
    match value {
        FieldValue::Text(s) => parse(s).ok_or(FieldError::Invalid("not one of its choices")),
        _ => Err(FieldError::Invalid("not one of its choices")),
    }
}

pub(super) fn color(value: &FieldValue) -> Result<Rgba, FieldError> {
    match value {
        FieldValue::Color(c) => Ok(*c),
        _ => Err(FieldError::Invalid("a color")),
    }
}

pub(super) fn colors(value: &FieldValue, min: usize, max: usize) -> Result<Vec<Rgba>, FieldError> {
    match value {
        FieldValue::Colors(list) if (min..=max).contains(&list.len()) => Ok(list.clone()),
        FieldValue::Colors(_) => Err(FieldError::Invalid("too few or too many colors")),
        _ => Err(FieldError::Invalid("a list of colors")),
    }
}
