//! The theme configurator: a theme's editable colors (backgrounds, text, accent,
//! syntax), its caret, its decorations and its own settings, and composing a new theme on
//! the structure of another. Neutral tones written directly in the base theme (borders,
//! hovers, gutter) are shifted proportionally onto the new background / text, and the
//! accent shades onto the new accent; the rest stays. The decorations and the settings come
//! read (from the app: the theme's format isn't this crate's) and are written back in
//! their own sections.

use kernel::Rgba;

use crate::backdrop::{BackdropKind, BackdropSpec, MAX_BLUR};
use crate::caret::{Blink, CaretShape, CaretSpec, Motion};
use crate::decorations::{
    BindTo, DecorationLayer, DecorationsSpec, FieldKind, FieldValue, hex, number_text,
};
use crate::options::{Label, OptionKind, ThemeOption, value_text};

/// The editable interface colors (the `[palette]` keys, shared by the shipped themes).
pub const UI_TOKENS: &[&str] = &["base", "card", "panel", "line", "fg", "muted", "code", "accent"];

/// The interface's state colors, editable when the theme's `[palette]` has them (an error's,
/// a warning's, a success's).
pub const STATE_TOKENS: &[&str] = &["red", "yellow", "green"];

/// The editable syntax colors (the `[syntax]` keys).
pub const SYNTAX_TOKENS: &[&str] =
    &["keyword", "string", "number", "comment", "function", "variable", "type", "property"];

/// The syntax keys that follow an editable color (the same color).
const SYNTAX_FOLLOW: &[(&str, &str)] =
    &[("function.method", "function"), ("variable.php", "variable"), ("property.json", "property")];

/// A theme in progress: the name, colors (key → color), caret, decorations and its own
/// settings (their defaults: what the studio shows chosen).
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeDraft {
    pub id: String,
    pub name: String,
    pub colors: Vec<(String, Rgba)>,
    pub caret: CaretSpec,
    /// What's under the windows over the editor (`[backdrop]`).
    pub backdrop: BackdropSpec,
    /// The floating panels' glass (`[floating]`: the menu drawer's).
    pub floating: crate::backdrop::SurfaceSpec,
    pub decorations: DecorationsSpec,
    pub options: Vec<ThemeOption>,
}

/// The key of the backdrop's color for [`ThemeDraft::get`] / [`ThemeDraft::set`] (black
/// while the theme doesn't say).
pub const BACKDROP_COLOR: &str = "backdrop";

/// The key of a decoration layer's color for [`ThemeDraft::get`] / [`ThemeDraft::set`]:
/// layer `layer`'s option `key`, its color `index`.
pub fn layer_color_key(layer: usize, key: &str, index: usize) -> String {
    format!("layer:{layer}:{key}:{index}")
}

/// The key of a color setting of the theme's own for [`ThemeDraft::get`] / [`ThemeDraft::set`].
pub fn option_color_key(id: &str) -> String {
    format!("option:{id}")
}

impl ThemeDraft {
    /// A color: an editable one (`base`, `syntax.string`), a layer's
    /// ([`layer_color_key`]) or a color setting's ([`option_color_key`]).
    pub fn get(&self, key: &str) -> Option<Rgba> {
        if key == BACKDROP_COLOR {
            return Some(self.backdrop.color.unwrap_or(Rgba::BLACK));
        }
        if let Some(rest) = key.strip_prefix("layer:") {
            let (layer, field, index) = split_layer_key(rest)?;
            return self.decorations.layers.get(layer)?.spec.color_at(field, index);
        }
        if let Some(id) = key.strip_prefix("option:") {
            return match &self.options.iter().find(|o| o.id == id)?.default {
                crate::options::OptionValue::Color(c) => Some(*c),
                _ => None,
            };
        }
        self.colors.iter().find(|(k, _)| k == key).map(|(_, c)| *c)
    }

    /// Sets a color (see [`Self::get`]); a layer's color set here follows nothing any more.
    pub fn set(&mut self, key: &str, color: Rgba) {
        if key == BACKDROP_COLOR {
            self.backdrop.color = Some(color.with_alpha(1.0));
            return;
        }
        if let Some(rest) = key.strip_prefix("layer:") {
            if let Some((i, field, index)) = split_layer_key(rest)
                && let Some(layer) = self.decorations.layers.get_mut(i)
                && layer.spec.set_color_at(field, index, color)
            {
                layer.unbind_at(field, index);
            }
            return;
        }
        if let Some(id) = key.strip_prefix("option:") {
            if let Some(o) = self.options.iter_mut().find(|o| o.id == id)
                && matches!(o.kind, OptionKind::Color { .. })
            {
                o.default = crate::options::OptionValue::Color(color);
            }
            return;
        }
        match self.colors.iter_mut().find(|(k, _)| k == key) {
            Some((_, c)) => *c = color,
            None => self.colors.push((key.to_owned(), color)),
        }
    }
}

fn split_layer_key(rest: &str) -> Option<(usize, &str, usize)> {
    let mut parts = rest.split(':');
    let layer = parts.next()?.parse().ok()?;
    let field = parts.next()?;
    let index = parts.next()?.parse().ok()?;
    Some((layer, field, index))
}

/// A theme the configurator may start from: its id, its name, its text, and what the app
/// read of it — its decorations and its own settings (through what it extends; their
/// defaults the values the user has now).
#[derive(Debug, Clone, PartialEq)]
pub struct StudioTheme {
    pub id: String,
    pub name: String,
    pub source: String,
    pub decorations: DecorationsSpec,
    pub options: Vec<ThemeOption>,
}

impl StudioTheme {
    /// Only its text (nothing read of its decorations or settings).
    pub fn plain(id: &str, name: &str, source: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            source: source.to_owned(),
            decorations: DecorationsSpec::default(),
            options: Vec::new(),
        }
    }

    /// Its draft: the colors and the caret from its text, the rest as read.
    pub fn draft(&self) -> Option<ThemeDraft> {
        let mut draft = read_draft(&self.source)?;
        draft.decorations = self.decorations.clone();
        draft.options = self.options.clone();
        Some(draft)
    }
}

/// What the configurator receives from the app: the themes and the active theme.
#[derive(Debug, Clone, PartialEq)]
pub struct StudioInput {
    pub themes: Vec<StudioTheme>,
    pub current: String,
}

/// The extension of the configurator's view (a virtual tab's): the theme studio module's
/// viewer opens it; the app closes it once a theme is saved.
pub const STUDIO_VIEW: &str = "view-theme";

/// The id of the preview theme (registered temporarily while editing).
pub const PREVIEW_ID: &str = "studio-preview";

/// The sections of a TOML theme (text): the header, `[palette]`, `[ui]`, `[syntax]`.
struct Sections<'a> {
    head: &'a str,
    palette: &'a str,
    ui: &'a str,
    syntax: &'a str,
}

fn sections(src: &str) -> Option<Sections<'_>> {
    let (head, rest) = src.split_once("\n[palette]\n")?;
    // `[caret]`, `[[options]]` and the decorations (anywhere after) aren't among them: cut
    // where the first of them starts.
    let cut = [
        "\n[caret]\n",
        "\n[backdrop]\n",
        "\n[floating]\n",
        "\n[decorations",
        "\n[[decorations.",
        "\n[[options",
    ]
    .iter()
    .filter_map(|marker| rest.find(marker))
    .min()
    .unwrap_or(rest.len());
    let rest = &rest[..cut];
    let (palette, rest) = rest.split_once("\n[ui]\n")?;
    let (ui, syntax) = rest.split_once("\n[syntax]\n")?;
    Some(Sections { head, palette, ui, syntax })
}

/// `key = "value"` → (key without quotes, the value between the quotes).
fn entry(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim().trim_matches('"');
    let value = value.trim();
    Some((key, value))
}

fn quoted(value: &str) -> Option<&str> {
    let start = value.find('"')? + 1;
    let end = start + value[start..].find('"')?;
    Some(&value[start..end])
}

fn palette_of(section: &str) -> Vec<(String, Rgba)> {
    section
        .lines()
        .filter_map(|l| {
            let (k, v) = entry(l)?;
            Some((k.to_owned(), Rgba::from_hex(quoted(v)?)?))
        })
        .collect()
}

fn resolve(value: &str, palette: &[(String, Rgba)]) -> Option<Rgba> {
    let v = if value.trim_start().starts_with('{') {
        let fg = value.find("fg")?;
        quoted(&value[fg..])?
    } else {
        quoted(value)?
    };
    match v.strip_prefix('$') {
        Some(name) => palette.iter().find(|(k, _)| k == name).map(|(_, c)| *c),
        None => Rgba::from_hex(v),
    }
}

/// A theme's `[caret]` (from its text; none: nothing said).
fn caret_of(src: &str) -> CaretSpec {
    let Some((_, rest)) = src.split_once("\n[caret]\n") else { return CaretSpec::default() };
    let body = rest.split("\n[").next().unwrap_or_default();
    let mut spec = CaretSpec::default();
    for (k, v) in body.lines().filter_map(entry) {
        let text = quoted(v).unwrap_or(v);
        match k {
            "shape" => spec.shape = CaretShape::parse(text),
            "width" => spec.width = text.parse().ok(),
            "blink" => spec.blink = Blink::parse(text),
            "glow" => spec.glow = text.parse().ok(),
            "trail" => spec.trail_ms = text.parse().ok(),
            "motion" => spec.motion = Motion::parse(text),
            _ => {}
        }
    }
    spec
}

/// A theme's `[backdrop]` (from its text; none: nothing said). A `$name` color is read
/// from its palette.
fn backdrop_of(src: &str, palette: &[(String, Rgba)]) -> BackdropSpec {
    spec_of(src, "\n[backdrop]\n", 0.0..=1.0, palette)
}

/// A theme's `[floating]` (the same parts; its opacity the panel's).
fn floating_of(src: &str, palette: &[(String, Rgba)]) -> BackdropSpec {
    spec_of(src, "\n[floating]\n", crate::backdrop::MIN_PANEL_OPACITY..=1.0, palette)
}

fn spec_of(
    src: &str,
    header: &str,
    opacities: std::ops::RangeInclusive<f32>,
    palette: &[(String, Rgba)],
) -> BackdropSpec {
    let Some((_, rest)) = src.split_once(header) else {
        return BackdropSpec::default();
    };
    let body = rest.split("\n[").next().unwrap_or_default();
    let mut spec = BackdropSpec::default();
    for (k, v) in body.lines().filter_map(entry) {
        let text = quoted(v).unwrap_or(v);
        match k {
            "style" => spec.style = BackdropKind::parse(text),
            "color" => spec.color = resolve(v, palette).map(|c| c.with_alpha(1.0)),
            "opacity" => spec.opacity = text.parse().ok().filter(|o| opacities.contains(o)),
            "blur" => spec.blur = text.parse().ok().filter(|b| (0.0..=MAX_BLUR).contains(b)),
            _ => {}
        }
    }
    spec
}

/// A backdrop as a theme's `[backdrop]` section (empty when nothing is said).
fn backdrop_section(spec: &BackdropSpec) -> String {
    spec_section("backdrop", spec)
}

/// A spec as a theme's `[<name>]` section (empty when nothing is said).
fn spec_section(name: &str, spec: &BackdropSpec) -> String {
    let mut out = Vec::new();
    if let Some(style) = spec.style {
        out.push(format!("style   = \"{}\"", style.id()));
    }
    if let Some(color) = spec.color {
        out.push(format!("color   = \"{}\"", hex(color.with_alpha(1.0))));
    }
    if let Some(opacity) = spec.opacity {
        out.push(format!("opacity = {}", number_text(opacity)));
    }
    if let Some(blur) = spec.blur {
        out.push(format!("blur    = {}", number_text(blur)));
    }
    if out.is_empty() { String::new() } else { format!("\n[{name}]\n{}\n", out.join("\n")) }
}

/// A caret as a theme's `[caret]` section (empty when nothing is said).
fn caret_section(spec: &CaretSpec) -> String {
    let mut out = Vec::new();
    if let Some(shape) = spec.shape {
        out.push(format!("shape  = \"{}\"", shape.id()));
    }
    if let Some(width) = spec.width {
        out.push(format!("width  = {width}"));
    }
    if let Some(blink) = spec.blink {
        out.push(format!("blink  = \"{}\"", blink.id()));
    }
    if let Some(glow) = spec.glow {
        out.push(format!("glow   = {glow}"));
    }
    if let Some(trail) = spec.trail_ms {
        out.push(format!("trail  = {trail}"));
    }
    if let Some(motion) = spec.motion {
        out.push(format!("motion = \"{}\"", motion.id()));
    }
    if out.is_empty() { String::new() } else { format!("\n[caret]\n{}\n", out.join("\n")) }
}

/// A TOML string (quoted, escaped).
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A label's lines (`label.<locale> = "…"`), under `key`.
fn label_lines(out: &mut Vec<String>, key: &str, label: &Label) {
    for (locale, text) in &label.0 {
        if crate::decorations::valid_name(locale) {
            out.push(format!("{key}.{locale} = {}", toml_string(text)));
        }
    }
}

/// A theme's own settings as its `[[options]]` sections (empty when it has none).
fn options_section(options: &[ThemeOption]) -> String {
    let mut out = String::new();
    for o in options {
        let mut lines =
            vec![format!("id = {}", toml_string(&o.id)), format!("kind = \"{}\"", o.kind.id())];
        label_lines(&mut lines, "label", &o.label);
        let default = match &o.default {
            crate::options::OptionValue::Bool(b) => b.to_string(),
            crate::options::OptionValue::Number(n) => number_text(*n),
            other => toml_string(&value_text(other)),
        };
        lines.push(format!("default = {default}"));
        match &o.kind {
            OptionKind::Slider { min, max, step } => {
                lines.push(format!("min = {}", number_text(*min)));
                lines.push(format!("max = {}", number_text(*max)));
                lines.push(format!("step = {}", number_text(*step)));
            }
            OptionKind::Color { palette: Some(name) } => {
                lines.push(format!("palette = {}", toml_string(name)));
            }
            _ => {}
        }
        out.push_str(&format!("\n[[options]]\n{}\n", lines.join("\n")));
        if let OptionKind::Choice(choices) = &o.kind {
            for c in choices {
                let mut lines = vec![format!("id = {}", toml_string(&c.id))];
                label_lines(&mut lines, "label", &c.label);
                for (name, color) in &c.palette {
                    lines.push(format!("palette.{name} = \"{}\"", hex(*color)));
                }
                out.push_str(&format!("\n[[options.choices]]\n{}\n", lines.join("\n")));
            }
        }
    }
    out
}

/// What a layer's value is written as where it follows something (`"$name"`, `"@id"`).
fn bound(layer: &DecorationLayer, key: &str, index: usize) -> Option<String> {
    Some(match layer.binding(key, index)? {
        BindTo::Palette(name) => format!("\"${name}\""),
        BindTo::Option(id) => format!("\"@{id}\""),
    })
}

/// Decorations as a theme's `[[decorations.layers]]` sections (empty when it has none).
fn decorations_section(spec: &DecorationsSpec) -> String {
    let mut out = String::new();
    for layer in &spec.layers {
        let kind = layer.spec.kind();
        let mut lines = vec![format!("kind = \"{}\"", kind.id())];
        if let Some(when) = &layer.when {
            lines.push(format!("when = {}", toml_string(&when.text())));
        }
        if !layer.enabled {
            lines.push("enabled = false".to_owned());
        }
        for f in kind.fields() {
            let Some(value) = layer.spec.get(f.key) else { continue };
            let text = match (f.kind, value) {
                (FieldKind::Colors { .. }, FieldValue::Colors(list)) => {
                    let items: Vec<String> = list
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            bound(layer, f.key, i).unwrap_or_else(|| format!("\"{}\"", hex(*c)))
                        })
                        .collect();
                    format!("[{}]", items.join(", "))
                }
                (_, FieldValue::Color(c)) => {
                    bound(layer, f.key, 0).unwrap_or_else(|| format!("\"{}\"", hex(c)))
                }
                (_, FieldValue::Number(v)) => {
                    bound(layer, f.key, 0).unwrap_or_else(|| number_text(v))
                }
                (_, FieldValue::Text(t)) => toml_string(&t),
                (_, FieldValue::Colors(_)) => continue,
            };
            lines.push(format!("{} = {text}", f.key));
        }
        out.push_str(&format!("\n[[decorations.layers]]\n{}\n", lines.join("\n")));
    }
    out
}

/// A theme's editable colors and caret (from its text; its decorations and settings come
/// read: [`StudioTheme::draft`]).
pub fn read_draft(src: &str) -> Option<ThemeDraft> {
    let s = sections(src)?;
    let palette = palette_of(s.palette);
    let mut colors = Vec::new();
    for key in UI_TOKENS {
        let c = palette.iter().find(|(k, _)| k == key).map(|(_, c)| *c)?;
        colors.push(((*key).to_owned(), c));
    }
    for key in STATE_TOKENS {
        if let Some((_, c)) = palette.iter().find(|(k, _)| k == key) {
            colors.push(((*key).to_owned(), *c));
        }
    }
    for key in SYNTAX_TOKENS {
        let line = s.syntax.lines().find(|l| entry(l).is_some_and(|(k, _)| k == *key));
        if let Some(c) = line.and_then(|l| resolve(entry(l)?.1, &palette)) {
            colors.push((format!("syntax.{key}"), c));
        }
    }
    let field = |name: &str| {
        s.head.lines().find_map(|l| {
            let (k, v) = entry(l)?;
            (k == name).then(|| quoted(v).map(str::to_owned)).flatten()
        })
    };
    Some(ThemeDraft {
        id: field("id")?,
        name: field("name")?,
        colors,
        caret: caret_of(src),
        backdrop: backdrop_of(src, &palette),
        floating: floating_of(src, &palette),
        decorations: DecorationsSpec::default(),
        options: Vec::new(),
    })
}

fn luminance(c: Rgba) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// How colorful `c` is: its brightest channel less its darkest (0 for a grey, a black).
fn chroma(c: Rgba) -> f32 {
    c.r.max(c.g).max(c.b) - c.r.min(c.g).min(c.b)
}

/// (hue 0..1, HSL saturation).
fn hue_sat(c: Rgba) -> (f32, f32) {
    let (h, s, _) = c.to_hsl();
    (h / 360.0, s)
}

/// Composes theme `draft` on the structure of theme `template` (its text).
pub fn compose(template: &str, draft: &ThemeDraft) -> Option<String> {
    let s = sections(template)?;
    let base = read_draft(template)?;
    let old = |k: &str| base.get(k);
    let new = |k: &str| draft.get(k).or_else(|| base.get(k));
    let (b0, f0, a0) = (old("card")?, old("fg")?, old("accent")?);
    let (nb, nf, na) = (new("card")?, new("fg")?, new("accent")?);
    let dark = luminance(nb) < 0.5;

    // The header: id, name, appearance.
    let head: Vec<String> = s
        .head
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| match entry(l).map(|(k, _)| k) {
            Some("id") => format!("id = \"{}\"", draft.id),
            Some("name") => format!("name = \"{}\"", draft.name.replace('"', "'")),
            Some("appearance") => {
                format!("appearance = \"{}\"", if dark { "dark" } else { "light" })
            }
            Some("extends") => String::new(),
            _ => l.to_owned(),
        })
        .collect();

    // The palette: editable colors replaced, the rest (the base's syntax colors) stays.
    let palette: Vec<String> = s
        .palette
        .lines()
        .map(|l| match entry(l) {
            Some((k, _)) if UI_TOKENS.contains(&k) || STATE_TOKENS.contains(&k) => match new(k) {
                Some(c) => format!("{k:<9} = \"{}\"", hex(c)),
                None => l.to_owned(),
            },
            _ => l.to_owned(),
        })
        .collect();

    // UI: colors written directly, shifted onto the new background / text / accent.
    // A near-black (or near-white) color is a background (or a text) whatever its hue: HSL
    // calls a near-black green "saturated", so the chroma decides too — never the accent.
    let (same_ground, same_accent) = (nb == b0 && nf == f0, na == a0);
    let remap = |c: Rgba| -> Rgba {
        let (h, sat) = hue_sat(c);
        if sat < 0.22 || chroma(c) < 0.25 {
            if same_ground {
                return c;
            }
            let span = luminance(f0) - luminance(b0);
            let t =
                if span.abs() < f32::EPSILON { 0.0 } else { (luminance(c) - luminance(b0)) / span };
            let mut out = nb.mix(nf, t).with_alpha(1.0);
            out.a = c.a;
            return out;
        }
        let (ha, _) = hue_sat(a0);
        if (h - ha).abs() < 0.06 && sat > 0.5 {
            if same_accent {
                return c;
            }
            let mut out = na;
            out.a = c.a;
            return out;
        }
        c
    };
    let ui: Vec<String> =
        s.ui.lines()
            .map(|l| {
                let mut out = String::with_capacity(l.len());
                let mut rest = l;
                while let Some(i) = rest.find('#') {
                    out.push_str(&rest[..i]);
                    let tail = &rest[i + 1..];
                    let digits = tail.bytes().take_while(u8::is_ascii_hexdigit).count();
                    if (digits == 6 || digits == 8) && !l.trim_start().starts_with('#') {
                        match Rgba::from_hex(&rest[i..i + 1 + digits]) {
                            Some(c) => out.push_str(&hex(remap(c))),
                            None => out.push_str(&rest[i..i + 1 + digits]),
                        }
                        rest = &tail[digits..];
                    } else {
                        out.push('#');
                        rest = tail;
                    }
                }
                out.push_str(rest);
                out
            })
            .collect();

    // Syntax: the editable colors (with the style kept: italic etc.).
    let syntax_color = |key: &str| -> Option<Rgba> {
        let key = SYNTAX_FOLLOW.iter().find(|(k, _)| *k == key).map_or(key, |(_, to)| to);
        SYNTAX_TOKENS.contains(&key).then(|| draft.get(&format!("syntax.{key}"))).flatten()
    };
    let syntax: Vec<String> = s
        .syntax
        .lines()
        .map(|l| {
            let Some((k, v)) = entry(l) else { return l.to_owned() };
            let Some(c) = syntax_color(k) else { return l.to_owned() };
            let value = if v.trim_start().starts_with('{') {
                match (v.find("fg"), v.find("fg").and_then(|i| quoted(&v[i..]))) {
                    (Some(i), Some(old)) => {
                        let at = i + v[i..].find(old).unwrap_or(0);
                        format!("{}{}{}", &v[..at], hex(c), &v[at + old.len()..])
                    }
                    _ => v.to_owned(),
                }
            } else {
                format!("\"{}\"", hex(c))
            };
            format!("\"{k}\" = {value}")
        })
        .collect();

    Some(format!(
        "# {}: made in the {} theme studio.\n{}\n\n[palette]\n{}\n\n[ui]\n{}\n\n[syntax]\n{}\n{}{}{}{}{}",
        draft.name,
        kernel::brand::NAME,
        head.join("\n").trim_matches('\n'),
        palette.join("\n").trim_matches('\n'),
        ui.join("\n").trim_matches('\n'),
        syntax.join("\n").trim_matches('\n'),
        caret_section(&draft.caret),
        backdrop_section(&draft.backdrop),
        spec_section("floating", &draft.floating),
        options_section(&draft.options),
        decorations_section(&draft.decorations),
    ))
}

#[cfg(test)]
#[path = "../tests/unit/studio.rs"]
mod tests;
