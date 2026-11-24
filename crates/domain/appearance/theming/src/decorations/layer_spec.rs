//! One layer's values: made with its defaults, read and set by option (checked against its
//! kind and limits), where it shows, whether it moves and what that costs, kept in its
//! limits.

use kernel::Rgba;

use super::fields::{amount, choice, color, colors, field};
use super::{
    CornerSpec, CornerStyle, Cost, EffectSpec, FieldError, FieldKind, FieldValue, GarlandEdges,
    GarlandSpec, GlowSpec, GlowStyle, Items, LayerKind, LayerSpec, MAX_DROP, MAX_GLOW_OPACITY,
    MAX_LIGHT_COLORS, MAX_ORNAMENT_COLORS, MAX_SNOW_OPACITY, ParticleShape, ParticleSpec, Strand,
    Zone, valid_name,
};

impl LayerSpec {
    /// A layer of `kind` with its defaults.
    pub fn new(kind: LayerKind) -> Self {
        match kind {
            LayerKind::Garland => Self::Garland(GarlandSpec::default()),
            LayerKind::Particles => Self::Particles(ParticleSpec::default()),
            LayerKind::Corners => Self::Corners(CornerSpec::default()),
            LayerKind::Glow => Self::Glow(GlowSpec::default()),
            kind => Self::Effect(EffectSpec::new(kind)),
        }
    }

    pub fn kind(&self) -> LayerKind {
        match self {
            Self::Garland(_) => LayerKind::Garland,
            Self::Particles(_) => LayerKind::Particles,
            Self::Corners(_) => LayerKind::Corners,
            Self::Glow(_) => LayerKind::Glow,
            Self::Effect(e) => e.kind,
        }
    }

    /// Where it shows (behind the content: around the panels, or also through them).
    pub fn zone(&self) -> Zone {
        match self {
            Self::Particles(p) => p.zone,
            // The neon edge is on the edges only.
            Self::Effect(e) if e.kind == LayerKind::Neon => Zone::Edges,
            // A retro screen never goes behind the editor (its lines would cross the text).
            Self::Effect(e) if e.kind.never_behind_editor() && e.zone == Zone::Everywhere => {
                Zone::Panels
            }
            Self::Effect(e) => e.zone,
            _ => Zone::Edges,
        }
    }

    /// How much of it shows through the panels (0..1; with a zone other than the edges).
    pub fn glass(&self) -> f32 {
        match self {
            Self::Particles(p) => p.glass,
            Self::Effect(e) => e.glass,
            _ => 0.0,
        }
    }

    /// The value of its option `key`.
    pub fn get(&self, key: &str) -> Option<FieldValue> {
        use FieldValue::{Color, Colors, Number, Text};
        let text = |s: &str| Some(Text(s.to_owned()));
        match self {
            Self::Garland(g) => match key {
                "strand" => text(g.strand.id()),
                "items" => text(g.items.id()),
                "edges" => text(g.edges.id()),
                "lights" => Some(Number(g.lights)),
                "twinkle" => Some(Number(g.twinkle)),
                "glow" => Some(Number(g.glow)),
                "colors" => Some(Colors(g.colors.clone())),
                "ornaments" => Some(Colors(g.ornaments.clone())),
                "wire" => Some(Color(g.wire)),
                "drop" => Some(Number(g.drop)),
                "opacity" => Some(Number(g.opacity)),
                _ => None,
            },
            Self::Particles(p) => match key {
                "shape" => text(p.shape.id()),
                "density" => Some(Number(p.density)),
                "speed" => Some(Number(p.speed)),
                "colors" => Some(Colors(p.colors.clone())),
                "opacity" => Some(Number(p.opacity)),
                "zone" => text(p.zone.id()),
                "glass" => Some(Number(p.glass)),
                _ => None,
            },
            Self::Effect(e) => {
                self.kind().field(key)?;
                match key {
                    "style" => text(e.style),
                    "colors" => Some(Colors(e.colors.clone())),
                    "density" => Some(Number(e.density)),
                    "speed" => Some(Number(e.speed)),
                    "size" => Some(Number(e.size)),
                    "glow" => Some(Number(e.glow)),
                    "opacity" => Some(Number(e.opacity)),
                    "zone" => text(e.zone.id()),
                    "glass" => Some(Number(e.glass)),
                    _ => None,
                }
            }
            Self::Corners(c) => match key {
                "style" => text(c.style.id()),
                "icon" => text(&c.icon),
                "colors" => Some(Colors(vec![c.color, c.accent])),
                "opacity" => Some(Number(c.opacity)),
                _ => None,
            },
            Self::Glow(g) => match key {
                "style" => text(g.style.id()),
                "color" => Some(Color(g.color)),
                "opacity" => Some(Number(g.opacity)),
                "width" => Some(Number(g.width)),
                _ => None,
            },
        }
    }

    /// Sets its option `key` (checked against the option's kind and limits). A particles
    /// layer also takes `color` (one color: the format before the layers had one).
    pub fn set(&mut self, key: &str, value: &FieldValue) -> Result<(), FieldError> {
        let Some(field) = self.kind().field(key).copied().or_else(|| {
            (self.kind() == LayerKind::Particles && key == "color")
                .then_some(field("color", FieldKind::Color))
        }) else {
            return Err(FieldError::Unknown);
        };
        let max = match field.kind {
            FieldKind::Amount(a) => a.max(),
            _ => 1.0,
        };
        let (min_n, max_n) = match field.kind {
            FieldKind::Colors { min, max } => (min, max),
            _ => (0, 0),
        };
        match self {
            Self::Garland(g) => match key {
                "strand" => g.strand = choice(value, Strand::parse)?,
                "items" => g.items = choice(value, Items::parse)?,
                "edges" => {
                    g.edges = choice(value, |s| {
                        // `sides`: an alias written by the first version of the format.
                        if s == "sides" {
                            Some(GarlandEdges::TopSides)
                        } else {
                            GarlandEdges::parse(s)
                        }
                    })?;
                }
                "lights" => g.lights = amount(value, max)?,
                "twinkle" => g.twinkle = amount(value, max)?,
                "glow" => g.glow = amount(value, max)?,
                "colors" => g.colors = colors(value, min_n, max_n)?,
                "ornaments" => g.ornaments = colors(value, min_n, max_n)?,
                "wire" => g.wire = color(value)?,
                "drop" => g.drop = amount(value, max)?,
                "opacity" => g.opacity = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Particles(p) => match key {
                "shape" => p.shape = choice(value, ParticleShape::parse)?,
                "density" => p.density = amount(value, max)?,
                "speed" => p.speed = amount(value, max)?,
                "colors" => p.colors = colors(value, min_n, max_n)?,
                "color" => p.colors = vec![color(value)?],
                "opacity" => p.opacity = amount(value, max)?,
                "zone" => p.zone = choice(value, Zone::parse)?,
                "glass" => p.glass = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Effect(e) => match key {
                "style" => {
                    let styles = e.styles();
                    e.style = match value {
                        FieldValue::Text(s) => styles
                            .iter()
                            .copied()
                            .find(|x| x == s)
                            .ok_or(FieldError::Invalid("not one of its choices"))?,
                        _ => return Err(FieldError::Invalid("not one of its choices")),
                    };
                }
                "colors" => e.colors = colors(value, min_n, max_n)?,
                "density" => e.density = amount(value, max)?,
                "speed" => e.speed = amount(value, max)?,
                "size" => e.size = amount(value, max)?,
                "glow" => e.glow = amount(value, max)?,
                "opacity" => e.opacity = amount(value, max)?,
                "zone" => {
                    let zone = choice(value, Zone::parse)?;
                    // Only the places its kind offers.
                    if !matches!(field.kind, FieldKind::Choice(ids) if ids.contains(&zone.id())) {
                        return Err(FieldError::Invalid("not one of its choices"));
                    }
                    e.zone = zone;
                }
                "glass" => e.glass = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Corners(c) => match key {
                "style" => c.style = choice(value, CornerStyle::parse)?,
                "icon" => match value {
                    FieldValue::Text(s) if valid_name(s) => c.icon = s.clone(),
                    _ => return Err(FieldError::Invalid("an icon's name")),
                },
                "colors" => {
                    let list = colors(value, min_n, max_n)?;
                    c.color = list[0];
                    if let Some(accent) = list.get(1) {
                        c.accent = *accent;
                    }
                }
                "opacity" => c.opacity = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
            Self::Glow(g) => match key {
                "style" => g.style = choice(value, GlowStyle::parse)?,
                "color" => g.color = color(value)?,
                "opacity" => g.opacity = amount(value, max)?,
                "width" => g.width = amount(value, max)?,
                _ => return Err(FieldError::Unknown),
            },
        }
        Ok(())
    }

    /// Color `index` of option `key` (a list's, or the color itself at 0).
    pub fn color_at(&self, key: &str, index: usize) -> Option<Rgba> {
        match self.get(key)? {
            FieldValue::Color(c) if index == 0 => Some(c),
            FieldValue::Colors(list) => list.get(index).copied(),
            _ => None,
        }
    }

    /// Sets color `index` of option `key` (a list's, or the color itself at 0). `false` if
    /// there's no such color.
    pub fn set_color_at(&mut self, key: &str, index: usize, c: Rgba) -> bool {
        let value = match self.get(key) {
            Some(FieldValue::Color(_)) if index == 0 => FieldValue::Color(c),
            Some(FieldValue::Colors(mut list)) if index < list.len() => {
                list[index] = c;
                FieldValue::Colors(list)
            }
            _ => return false,
        };
        self.set(key, &value).is_ok()
    }

    /// Whether anything in it moves (glinting items, falling or glowing particles).
    pub fn moves(&self) -> bool {
        match self {
            Self::Garland(g) => g.twinkle > 0.0 && g.has_items() && g.items.glints(),
            Self::Particles(p) => p.density > 0.0 && (p.shape.falls() || p.speed > 0.0),
            Self::Corners(_) | Self::Glow(_) => false,
            Self::Effect(e) => match e.kind {
                LayerKind::Aurora | LayerKind::Neon | LayerKind::Synthwave => e.speed > 0.0,
                // Rare strikes (glitches): something happens now and then, whatever the speed.
                LayerKind::Lightning | LayerKind::Crt | LayerKind::Glitch => e.density > 0.0,
                _ => e.density > 0.0 && e.speed > 0.0,
            },
        }
    }

    /// What it costs to draw while it moves, roughly (the studio says it beside each
    /// effect, and adds them up).
    pub fn cost(&self) -> Cost {
        match self {
            Self::Corners(_) | Self::Glow(_) => Cost::Light,
            Self::Garland(g) if g.twinkle > 0.0 && g.has_items() => Cost::Medium,
            Self::Garland(_) => Cost::Light,
            Self::Particles(p) if p.density > 0.5 => Cost::Medium,
            Self::Particles(_) => Cost::Light,
            Self::Effect(e) => match e.kind {
                LayerKind::Matrix
                | LayerKind::Fireworks
                | LayerKind::Constellation
                | LayerKind::LavaLamp => Cost::Medium,
                LayerKind::Starfield | LayerKind::NeonRain if e.density > 0.5 => Cost::Medium,
                _ => Cost::Light,
            },
        }
    }

    /// The values kept in their limits (what a reader already checked, made safe for any
    /// caller: a layer built in code, the studio, the settings).
    pub fn clamped(mut self) -> Self {
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        match &mut self {
            Self::Garland(g) => {
                g.lights = unit(g.lights);
                g.twinkle = unit(g.twinkle);
                g.glow = unit(g.glow);
                g.drop = unit(g.drop).min(MAX_DROP);
                g.opacity = unit(g.opacity);
                g.colors.truncate(MAX_LIGHT_COLORS);
                if g.colors.is_empty() {
                    g.colors = GarlandSpec::default().colors;
                }
                g.ornaments.truncate(MAX_ORNAMENT_COLORS);
            }
            Self::Particles(p) => {
                p.density = unit(p.density);
                p.speed = unit(p.speed);
                p.opacity = unit(p.opacity).min(MAX_SNOW_OPACITY);
                p.colors.truncate(MAX_LIGHT_COLORS);
                if p.colors.is_empty() {
                    p.colors = ParticleSpec::default().colors;
                }
            }
            Self::Corners(c) => {
                c.opacity = unit(c.opacity);
                if !valid_name(&c.icon) {
                    c.icon = CornerSpec::default().icon;
                }
            }
            Self::Glow(g) => {
                g.opacity = unit(g.opacity).min(MAX_GLOW_OPACITY);
                g.width = unit(g.width);
            }
            Self::Effect(e) => {
                e.density = unit(e.density);
                e.speed = unit(e.speed);
                e.size = unit(e.size);
                e.glow = unit(e.glow);
                e.glass = unit(e.glass);
                e.opacity = unit(e.opacity).min(e.max_opacity());
                e.colors.truncate(MAX_LIGHT_COLORS);
                if e.colors.is_empty() {
                    e.colors = EffectSpec::new(e.kind).colors;
                }
                if e.kind.never_behind_editor() && e.zone == Zone::Everywhere {
                    e.zone = Zone::Panels;
                }
                if !e.styles().contains(&e.style) {
                    e.style = e.styles().first().copied().unwrap_or("");
                }
            }
        }
        if let Self::Particles(p) = &mut self {
            p.glass = unit(p.glass);
        }
        self
    }
}
