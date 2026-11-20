//! Module with the bundled themes (dark, light, Midnight, Arctic, Ember, Paper, Fjord,
//! Christmas…) and the theme file reader, contributed as a [`ThemeProvider`] (the app discovers it like any
//! module's contribution).
//!
//! The TOML format is described in docs/plan/07-teme.md. The same function [`parse`] also
//! reads user themes (the config folder's `themes/*.toml`), so a new theme means just a
//! new file, no code.

use std::collections::HashMap;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::Rgba;
use serde::Deserialize;
use theming::backdrop::{BackdropKind, BackdropSpec, MAX_BLUR, MAX_OPACITY};
use theming::caret::{Blink, CaretShape, CaretSpec, Motion};
use theming::catalog::ThemeProvider;
use theming::decorations::{
    BindTo, Binding, Condition, DecorationLayer, DecorationsSpec, FieldError, FieldKind,
    FieldValue, LayerKind, LayerSpec, MAX_LAYERS, Merge, valid_name,
};
use theming::options::{Label, MAX_OPTIONS, OptionChoice, OptionKind, OptionValue, ThemeOption};
use theming::{Appearance, TextStyle, Theme, ThemeId};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("themes"),
    name: "Themes",
    description: "The bundled themes and the theme files' format (the user's themes too)",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct ThemesModule;

impl Module for ThemesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<std::sync::Arc<dyn ThemeProvider>>(std::sync::Arc::new(Bundled));
        Ok(())
    }
}

/// The bundled themes and the TOML format.
#[derive(Debug)]
pub struct Bundled;

impl ThemeProvider for Bundled {
    fn bundled(&self) -> Vec<&'static str> {
        builtin_sources().collect()
    }

    fn parse(&self, source: &str) -> Result<Theme, String> {
        parse(source).map_err(|e| e.to_string())
    }
}

/// Themes bundled with the app, in the order they appear when cycling.
const BUILTIN: &[(&str, &str)] = &[
    ("default-dark.toml", include_str!("../../../../../themes/default-dark.toml")),
    ("default-light.toml", include_str!("../../../../../themes/default-light.toml")),
    ("midnight.toml", include_str!("../../../../../themes/midnight.toml")),
    ("arctic.toml", include_str!("../../../../../themes/arctic.toml")),
    ("ember.toml", include_str!("../../../../../themes/ember.toml")),
    ("paper.toml", include_str!("../../../../../themes/paper.toml")),
    ("dracula.toml", include_str!("../../../../../themes/dracula.toml")),
    ("grove.toml", include_str!("../../../../../themes/grove.toml")),
    ("neon.toml", include_str!("../../../../../themes/neon.toml")),
    ("mocha.toml", include_str!("../../../../../themes/mocha.toml")),
    ("retro.toml", include_str!("../../../../../themes/retro.toml")),
    ("onyx.toml", include_str!("../../../../../themes/onyx.toml")),
    ("ocean.toml", include_str!("../../../../../themes/ocean.toml")),
    ("ink.toml", include_str!("../../../../../themes/ink.toml")),
    ("rose.toml", include_str!("../../../../../themes/rose.toml")),
    ("solar-dark.toml", include_str!("../../../../../themes/solar-dark.toml")),
    ("matrix.toml", include_str!("../../../../../themes/matrix.toml")),
    ("cyberpunk.toml", include_str!("../../../../../themes/cyberpunk.toml")),
    ("latte.toml", include_str!("../../../../../themes/latte.toml")),
    ("solar-light.toml", include_str!("../../../../../themes/solar-light.toml")),
    ("sakura.toml", include_str!("../../../../../themes/sakura.toml")),
    ("mint.toml", include_str!("../../../../../themes/mint.toml")),
    ("daylight.toml", include_str!("../../../../../themes/daylight.toml")),
    ("fjord.toml", include_str!("../../../../../themes/fjord.toml")),
    ("dusk.toml", include_str!("../../../../../themes/dusk.toml")),
    ("slate.toml", include_str!("../../../../../themes/slate.toml")),
    ("linen.toml", include_str!("../../../../../themes/linen.toml")),
    ("lavender.toml", include_str!("../../../../../themes/lavender.toml")),
    ("christmas-dark.toml", include_str!("../../../../../themes/christmas-dark.toml")),
    ("christmas-light.toml", include_str!("../../../../../themes/christmas-light.toml")),
];

/// Text of the bundled themes (templates for the theme configurator).
pub fn builtin_sources() -> impl Iterator<Item = &'static str> {
    BUILTIN.iter().map(|(_, src)| *src)
}

pub fn builtin_themes() -> Vec<Theme> {
    BUILTIN
        .iter()
        .filter_map(|(name, src)| {
            parse(src)
                .inspect_err(|e| tracing::error!(%name, error = %e, "a bundled theme is invalid"))
                .ok()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeFileError {
    #[error("TOML invalid: {0}")]
    Syntax(String),
    #[error("invalid color `{value}` at `{key}` (formats: #rgb, #rrggbb, #rrggbbaa, $name)")]
    Color { key: String, value: String },
    #[error("unknown appearance `{0}` (light or dark)")]
    Appearance(String),
    #[error("invalid caret `{key} = {value}`")]
    Caret { key: &'static str, value: String },
    #[error("invalid backdrop `{key} = {value}`")]
    Backdrop { key: &'static str, value: String },
    #[error("invalid decoration `{key} = {value}` ({why})")]
    Decoration { key: String, value: String, why: &'static str },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    id: String,
    name: String,
    appearance: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    palette: HashMap<String, String>,
    #[serde(default)]
    ui: HashMap<String, String>,
    #[serde(default)]
    syntax: HashMap<String, SyntaxEntry>,
    #[serde(default)]
    caret: CaretEntry,
    #[serde(default)]
    backdrop: BackdropEntry,
    #[serde(default)]
    floating: BackdropEntry,
    #[serde(default)]
    options: Vec<toml::Table>,
    #[serde(default)]
    decorations: DecorationsEntry,
}

/// `[decorations]`: the layers (`[[decorations.layers]]`) or the first format's sections
/// (`[decorations.garland]`, `.snow`, `.corners`), each a table of its options (checked by
/// the kind's fields; an unknown kind or option is an error).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecorationsEntry {
    layers: Option<Vec<toml::Table>>,
    garland: Option<toml::Table>,
    snow: Option<toml::Table>,
    corners: Option<toml::Table>,
}

/// What a theme's values are read with: its palette (`$name`) and its own settings
/// (`@id`), and where what was ignored is noted.
struct Refs<'a> {
    palette: &'a HashMap<String, String>,
    options: &'a [ThemeOption],
    notes: &'a mut Vec<String>,
}

impl Refs<'_> {
    /// A color: `#…`, `$name` (and the name it follows) or `@option` (a color setting's).
    fn color(&mut self, key: &str, value: &str) -> Result<(Rgba, Option<BindTo>), ThemeFileError> {
        let bad = || ThemeFileError::Color { key: key.to_owned(), value: value.to_owned() };
        if let Some(id) = value.strip_prefix('@') {
            return match self.options.iter().find(|o| o.id == id).map(|o| &o.default) {
                Some(OptionValue::Color(c)) => Ok((*c, Some(BindTo::Option(id.to_owned())))),
                _ => Err(bad()),
            };
        }
        match value.strip_prefix('$') {
            Some(name) => {
                let hex = self.palette.get(name).ok_or_else(bad)?;
                let c = Rgba::from_hex(hex).ok_or_else(bad)?;
                Ok((c, Some(BindTo::Palette(name.to_owned()))))
            }
            None => Ok((Rgba::from_hex(value).ok_or_else(bad)?, None)),
        }
    }
}

fn bad_decoration(key: &str, value: &toml::Value, why: &'static str) -> ThemeFileError {
    let value = match value {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    ThemeFileError::Decoration { key: key.to_owned(), value, why }
}

/// One layer from its table: `kind` (unless the first format's section names it), `when`,
/// every other key one of the kind's options.
fn read_layer(
    name: &str,
    kind: LayerKind,
    table: &toml::Table,
    refs: &mut Refs<'_>,
) -> Result<DecorationLayer, ThemeFileError> {
    let mut layer = DecorationLayer::plain(LayerSpec::new(kind));
    for (key, value) in table {
        let at = format!("{name}.{key}");
        match key.as_str() {
            "kind" => continue,
            "enabled" => {
                layer.enabled =
                    value.as_bool().ok_or_else(|| bad_decoration(&at, value, "true or false"))?;
                continue;
            }
            "when" => {
                let text = value.as_str().unwrap_or_default();
                let when = Condition::parse(text)
                    .ok_or_else(|| bad_decoration(&at, value, "an option's name"))?;
                if !refs.options.iter().any(|o| o.id == when.option) {
                    refs.notes.push(format!("{at}: no option `{}` (drawn always)", when.option));
                }
                layer.when = Some(when);
                continue;
            }
            _ => {}
        }
        let field = kind.field(key).copied().or_else(|| {
            (kind == LayerKind::Particles && key == "color")
                .then_some(theming::decorations::Field { key: "color", kind: FieldKind::Color })
        });
        let Some(field) = field else {
            return Err(ThemeFileError::Syntax(format!("unknown decoration option `{at}`")));
        };
        let value = match (field.kind, value) {
            (FieldKind::Choice(_) | FieldKind::Text, toml::Value::String(s)) => {
                FieldValue::Text(s.clone())
            }
            (FieldKind::Amount(_), toml::Value::Integer(n)) => FieldValue::Number(*n as f32),
            (FieldKind::Amount(_), toml::Value::Float(n)) => FieldValue::Number(*n as f32),
            (FieldKind::Amount(_), toml::Value::String(s)) if s.starts_with('@') => {
                let id = &s[1..];
                match refs.options.iter().find(|o| o.id == id).map(|o| &o.default) {
                    Some(OptionValue::Number(n)) => {
                        layer.bindings.push(Binding {
                            key: field.key,
                            index: 0,
                            to: BindTo::Option(id.to_owned()),
                        });
                        FieldValue::Number(*n)
                    }
                    _ => {
                        refs.notes
                            .push(format!("{at}: no slider option `{id}` (its default kept)"));
                        continue;
                    }
                }
            }
            (FieldKind::Color, toml::Value::String(s)) => {
                let (c, to) = refs.color(&at, s)?;
                if let Some(to) = to {
                    layer.bindings.push(Binding { key: field.key, index: 0, to });
                }
                FieldValue::Color(c)
            }
            (FieldKind::Colors { .. }, toml::Value::Array(list)) => {
                let mut colors = Vec::with_capacity(list.len());
                for (i, item) in list.iter().enumerate() {
                    let text = item.as_str().ok_or_else(|| bad_decoration(&at, item, "a color"))?;
                    let (c, to) = refs.color(&at, text)?;
                    if let Some(to) = to {
                        layer.bindings.push(Binding { key: field.key, index: i, to });
                    }
                    colors.push(c);
                }
                FieldValue::Colors(colors)
            }
            (_, other) => return Err(bad_decoration(&at, other, "not a value it takes")),
        };
        match layer.spec.set(field.key, &value) {
            Ok(()) => {}
            Err(FieldError::Unknown) => {
                return Err(ThemeFileError::Syntax(format!("unknown decoration option `{at}`")));
            }
            Err(FieldError::Invalid(why)) => return Err(bad_decoration(&at, &table[key], why)),
        }
    }
    Ok(layer)
}

impl DecorationsEntry {
    fn spec(&self, refs: &mut Refs<'_>) -> Result<DecorationsSpec, ThemeFileError> {
        if let Some(tables) = &self.layers {
            if self.garland.is_some() || self.snow.is_some() || self.corners.is_some() {
                return Err(ThemeFileError::Syntax(
                    "`[[decorations.layers]]` and the sections of the first format together"
                        .to_owned(),
                ));
            }
            if tables.len() > MAX_LAYERS {
                return Err(ThemeFileError::Decoration {
                    key: "layers".to_owned(),
                    value: format!("{} layers", tables.len()),
                    why: "too many layers",
                });
            }
            let mut layers = Vec::with_capacity(tables.len());
            for (i, table) in tables.iter().enumerate() {
                let name = format!("layers[{i}]");
                let kind = table
                    .get("kind")
                    .and_then(toml::Value::as_str)
                    .and_then(LayerKind::parse)
                    .ok_or_else(|| ThemeFileError::Decoration {
                        key: format!("{name}.kind"),
                        value: table.get("kind").map(ToString::to_string).unwrap_or_default(),
                        why: "one of the decorations' kinds",
                    })?;
                layers.push(read_layer(&name, kind, table, refs)?);
            }
            return Ok(DecorationsSpec { layers, merge: Merge::Replace });
        }
        // The first format: drawn in the order it drew them (the garland, the corners, the
        // snow over them).
        let mut layers = Vec::new();
        for (name, kind, table) in [
            ("garland", LayerKind::Garland, &self.garland),
            ("corners", LayerKind::Corners, &self.corners),
            ("snow", LayerKind::Particles, &self.snow),
        ] {
            if let Some(table) = table {
                layers.push(read_layer(name, kind, table, refs)?);
            }
        }
        let merge = if layers.is_empty() { Merge::Inherit } else { Merge::ByKind };
        Ok(DecorationsSpec { layers, merge })
    }
}

/// A label: one text, or one per language (`label.en`, `label.ro`).
fn read_label(value: Option<&toml::Value>) -> Option<Label> {
    match value? {
        toml::Value::String(s) => Some(Label::plain(s)),
        toml::Value::Table(t) => {
            let texts: Vec<(String, String)> = t
                .iter()
                .filter_map(|(locale, text)| Some((locale.clone(), text.as_str()?.to_owned())))
                .collect();
            (!texts.is_empty()).then_some(Label(texts))
        }
        _ => None,
    }
}

fn number(value: Option<&toml::Value>) -> Option<f32> {
    match value? {
        toml::Value::Integer(n) => Some(*n as f32),
        toml::Value::Float(n) => Some(*n as f32),
        _ => None,
    }
}

/// One of the theme's own settings (`None`, with a note, when it can't be one).
fn read_option(
    i: usize,
    table: &toml::Table,
    palette: &HashMap<String, String>,
    notes: &mut Vec<String>,
) -> Option<ThemeOption> {
    let id = table.get("id").and_then(toml::Value::as_str).filter(|id| valid_name(id));
    let Some(id) = id else {
        notes.push(format!("options[{i}]: no valid `id` (ignored)"));
        return None;
    };
    macro_rules! skip {
        ($why:expr) => {{
            notes.push(format!("option `{id}`: {} (ignored)", $why));
            return None;
        }};
    }
    const KEYS: &[&str] =
        &["id", "kind", "label", "default", "choices", "min", "max", "step", "palette"];
    for key in table.keys().filter(|k| !KEYS.contains(&k.as_str())) {
        notes.push(format!("option `{id}`: unknown key `{key}` (ignored)"));
    }
    let label = read_label(table.get("label")).unwrap_or_else(|| Label::plain(id));
    let color = |text: &str| match text.strip_prefix('$') {
        Some(name) => palette.get(name).and_then(|hex| Rgba::from_hex(hex)),
        None => Rgba::from_hex(text),
    };
    let default = table.get("default");
    let (kind, default) = match table.get("kind").and_then(toml::Value::as_str) {
        Some("toggle") => (
            OptionKind::Toggle,
            OptionValue::Bool(default.and_then(toml::Value::as_bool).unwrap_or(false)),
        ),
        Some("choice") => {
            let Some(list) = table.get("choices").and_then(toml::Value::as_array) else {
                skip!("a choice without `choices`")
            };
            let mut choices: Vec<OptionChoice> = Vec::new();
            for item in list {
                let Some(t) = item.as_table() else { continue };
                let Some(cid) = t.get("id").and_then(toml::Value::as_str).filter(|c| valid_name(c))
                else {
                    notes.push(format!("option `{id}`: a choice without a valid `id` (ignored)"));
                    continue;
                };
                if choices.iter().any(|c| c.id == cid) {
                    continue;
                }
                let mut changes = Vec::new();
                for (name, value) in
                    t.get("palette").and_then(toml::Value::as_table).into_iter().flatten()
                {
                    match value.as_str().and_then(color) {
                        Some(c) if palette.contains_key(name) => changes.push((name.clone(), c)),
                        _ => notes.push(format!(
                            "option `{id}`: choice `{cid}` can't change `{name}` (ignored)"
                        )),
                    }
                }
                choices.push(OptionChoice {
                    id: cid.to_owned(),
                    label: read_label(t.get("label")).unwrap_or_else(|| Label::plain(cid)),
                    palette: changes,
                });
            }
            if choices.is_empty() {
                skip!("no choices")
            }
            let chosen = default
                .and_then(toml::Value::as_str)
                .filter(|d| choices.iter().any(|c| c.id == *d))
                .unwrap_or(&choices[0].id)
                .to_owned();
            (OptionKind::Choice(choices), OptionValue::Choice(chosen))
        }
        Some("slider") => {
            let (min, max) =
                (number(table.get("min")).unwrap_or(0.0), number(table.get("max")).unwrap_or(1.0));
            let step = number(table.get("step")).unwrap_or((max - min) / 10.0);
            if !(min.is_finite() && max.is_finite() && step.is_finite())
                || min >= max
                || step <= 0.0
            {
                skip!("a slider needs min < max and a step above 0")
            }
            let value = number(default).filter(|v| (min..=max).contains(v)).unwrap_or(min);
            (OptionKind::Slider { min, max, step }, OptionValue::Number(value))
        }
        Some("color") => {
            let target = table.get("palette").and_then(toml::Value::as_str);
            if let Some(name) = target
                && !palette.contains_key(name)
            {
                skip!("its `palette` isn't a name of the palette")
            }
            let value = default
                .and_then(toml::Value::as_str)
                .and_then(color)
                .or_else(|| target.and_then(|n| palette.get(n)).and_then(|h| Rgba::from_hex(h)));
            let Some(value) = value else { skip!("a color without a valid `default`") };
            (OptionKind::Color { palette: target.map(str::to_owned) }, OptionValue::Color(value))
        }
        Some(other) => skip!(format!("unknown kind `{other}`")),
        None => skip!("no `kind`"),
    };
    Some(ThemeOption { id: id.to_owned(), label, kind, default })
}

/// The theme's own settings (`[[options]]`): the ones that can't be are left out, noted.
fn read_options(
    tables: &[toml::Table],
    palette: &HashMap<String, String>,
    notes: &mut Vec<String>,
) -> Vec<ThemeOption> {
    let mut out: Vec<ThemeOption> = Vec::new();
    for (i, table) in tables.iter().enumerate() {
        let Some(option) = read_option(i, table, palette, notes) else { continue };
        if out.iter().any(|o| o.id == option.id) {
            notes.push(format!("option `{}`: declared twice (the second ignored)", option.id));
        } else if out.len() == MAX_OPTIONS {
            notes.push(format!("option `{}`: too many options (ignored)", option.id));
        } else {
            out.push(option);
        }
    }
    out
}

/// `[caret]`: how the caret looks and moves (each optional).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaretEntry {
    shape: Option<String>,
    width: Option<f32>,
    blink: Option<String>,
    glow: Option<f32>,
    trail: Option<u32>,
    motion: Option<String>,
}

impl CaretEntry {
    fn spec(&self) -> Result<CaretSpec, ThemeFileError> {
        fn read<T>(
            key: &'static str,
            value: Option<&String>,
            parse: fn(&str) -> Option<T>,
        ) -> Result<Option<T>, ThemeFileError> {
            value
                .map(|v| parse(v).ok_or_else(|| ThemeFileError::Caret { key, value: v.clone() }))
                .transpose()
        }
        Ok(CaretSpec {
            shape: read("shape", self.shape.as_ref(), CaretShape::parse)?,
            width: self.width,
            blink: read("blink", self.blink.as_ref(), Blink::parse)?,
            glow: self.glow,
            trail_ms: self.trail,
            motion: read("motion", self.motion.as_ref(), Motion::parse)?,
        })
    }
}

/// `[backdrop]`: what's under the windows over the editor (each optional): `style`
/// (simple, glass), `color` (`#…` or `$name`), `opacity` (0–1), `blur` (logical pixels,
/// 0–12).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackdropEntry {
    style: Option<String>,
    color: Option<String>,
    opacity: Option<f32>,
    blur: Option<f32>,
}

impl BackdropEntry {
    fn spec(
        &self,
        color: &dyn Fn(&str, &str) -> Result<Rgba, ThemeFileError>,
    ) -> Result<BackdropSpec, ThemeFileError> {
        self.read("backdrop.color", 0.0..=1.0, MAX_OPACITY, color)
    }

    /// `[floating]`: the same parts, the opacity the panel's
    /// ([`MIN_PANEL_OPACITY`](theming::backdrop::MIN_PANEL_OPACITY)–1).
    fn surface(
        &self,
        color: &dyn Fn(&str, &str) -> Result<Rgba, ThemeFileError>,
    ) -> Result<theming::backdrop::SurfaceSpec, ThemeFileError> {
        self.read("floating.color", theming::backdrop::MIN_PANEL_OPACITY..=1.0, 1.0, color)
    }

    fn read(
        &self,
        color_key: &str,
        opacities: std::ops::RangeInclusive<f32>,
        most: f32,
        color: &dyn Fn(&str, &str) -> Result<Rgba, ThemeFileError>,
    ) -> Result<BackdropSpec, ThemeFileError> {
        let bad = |key, value: &str| ThemeFileError::Backdrop { key, value: value.to_owned() };
        let style = match self.style.as_deref() {
            Some(s) => Some(BackdropKind::parse(s).ok_or_else(|| bad("style", s))?),
            None => None,
        };
        let blur = match self.blur {
            Some(b) if (0.0..=MAX_BLUR).contains(&b) => Some(b),
            Some(b) => return Err(bad("blur", &b.to_string())),
            None => None,
        };
        let opacity = match self.opacity {
            Some(o) if opacities.contains(&o) => Some(o.min(most)),
            Some(o) => return Err(bad("opacity", &o.to_string())),
            None => None,
        };
        let color = self
            .color
            .as_deref()
            .map(|c| color(color_key, c).map(|c| c.with_alpha(1.0)))
            .transpose()?;
        Ok(BackdropSpec { style, color, opacity, blur })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SyntaxEntry {
    Color(String),
    Style {
        #[serde(default)]
        fg: Option<String>,
        #[serde(default)]
        bg: Option<String>,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        italic: bool,
    },
}

/// Reads a theme from TOML.
pub fn parse(source: &str) -> Result<Theme, ThemeFileError> {
    let file: ThemeFile =
        toml::from_str(source).map_err(|e| ThemeFileError::Syntax(e.to_string()))?;
    let appearance = match file.appearance.as_str() {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        other => return Err(ThemeFileError::Appearance(other.to_owned())),
    };
    let color = |key: &str, value: &str| -> Result<Rgba, ThemeFileError> {
        let resolved = match value.strip_prefix('$') {
            Some(name) => file.palette.get(name).map(String::as_str).unwrap_or(value),
            None => value,
        };
        Rgba::from_hex(resolved)
            .ok_or_else(|| ThemeFileError::Color { key: key.to_owned(), value: value.to_owned() })
    };

    // The palette's name each token and scope was written with (a setting may change it).
    let name_of = |value: &str| {
        value.strip_prefix('$').filter(|n| file.palette.contains_key(*n)).map(str::to_owned)
    };
    let ui_refs: HashMap<String, String> =
        file.ui.iter().filter_map(|(k, v)| Some((k.clone(), name_of(v)?))).collect();
    let syntax_refs: HashMap<String, String> = file
        .syntax
        .iter()
        .filter_map(|(k, entry)| {
            let fg = match entry {
                SyntaxEntry::Color(c) => Some(c.as_str()),
                SyntaxEntry::Style { fg, .. } => fg.as_deref(),
            };
            Some((k.clone(), name_of(fg?)?))
        })
        .collect();
    let ui = file
        .ui
        .iter()
        .map(|(k, v)| Ok((k.clone(), color(k, v)?)))
        .collect::<Result<HashMap<_, _>, ThemeFileError>>()?;
    let syntax = file
        .syntax
        .iter()
        .map(|(k, entry)| {
            let style = match entry {
                SyntaxEntry::Color(c) => {
                    TextStyle { foreground: Some(color(k, c)?), ..TextStyle::default() }
                }
                SyntaxEntry::Style { fg, bg, bold, italic } => TextStyle {
                    foreground: fg.as_deref().map(|c| color(k, c)).transpose()?,
                    background: bg.as_deref().map(|c| color(k, c)).transpose()?,
                    bold: *bold,
                    italic: *italic,
                },
            };
            Ok((k.clone(), style))
        })
        .collect::<Result<HashMap<_, _>, ThemeFileError>>()?;

    let mut notes = Vec::new();
    let options = read_options(&file.options, &file.palette, &mut notes);
    let decorations = file.decorations.spec(&mut Refs {
        palette: &file.palette,
        options: &options,
        notes: &mut notes,
    })?;
    Ok(Theme {
        id: ThemeId::parse(&file.id),
        name: file.name,
        appearance,
        extends: file.extends.as_deref().map(ThemeId::parse),
        ui,
        syntax,
        caret: file.caret.spec()?,
        backdrop: file.backdrop.spec(&color)?,
        floating: file.floating.surface(&color)?,
        decorations,
        options,
        ui_refs,
        syntax_refs,
        notes,
    })
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
