//! Code colors module: finds colors written as hex (`#e5484d`), `rgb()` /
//! `rgba()`, `hsl()` / `hsla()` or CSS names (`red`), for the gutter swatch, and
//! writes them back in the same form after the color picker.
//!
//! Color names are recognized only as the value of a CSS property (`color: red`),
//! wherever it appears: in CSS, in an HTML `style="…"` or in a PHP / JS string.

use code_intelligence::{ColorFormat, ColorLiteral, ColorProvider};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::Rgba;
use std::sync::Arc;

mod named;
mod picker_view;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("editor.colors"),
    name: "Colors",
    description: "The color's swatch in the gutter and the color picker (hex, rgb, hsl, CSS names)",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct ColorsModule;

impl Module for ColorsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ColorProvider>>(Arc::new(Colors));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            picker_view::PickerProvider,
        ));
        Ok(())
    }
}

/// Languages in which colors are searched.
const LANGUAGES: &[&str] = &[
    "css",
    "scss",
    "sass",
    "less",
    "html",
    "twig",
    "blade",
    "php",
    "javascript",
    "typescript",
    "tsx",
    "jsx",
    "vue",
    "svelte",
    "json",
    "yaml",
];

#[derive(Debug)]
pub(crate) struct Colors;

impl ColorProvider for Colors {
    fn find(&self, language: &str, line: &str) -> Vec<ColorLiteral> {
        if !LANGUAGES.contains(&language) {
            return Vec::new();
        }
        let css = matches!(language, "css" | "scss" | "sass" | "less");
        find(line, css)
    }

    fn parse(&self, text: &str) -> Option<ColorLiteral> {
        let text = text.trim();
        let literal = literal_at(text, 0).or_else(|| {
            named::lookup(&text.to_ascii_lowercase()).map(|color| ColorLiteral {
                range: 0..text.len(),
                color,
                format: ColorFormat::Named,
            })
        })?;
        (literal.range.end == text.len()).then_some(literal)
    }

    fn format(&self, color: Rgba, format: ColorFormat) -> String {
        format_color(color, format)
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '$' | '@' | '.')
}

fn find(line: &str, css: bool) -> Vec<ColorLiteral> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if !line.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let before = line[..i].chars().next_back();
        let boundary = before.is_none_or(|c| !is_word(c) && c != '&' && c != '#');
        if boundary && let Some(literal) = literal_at(line, i) {
            // `#fade {` in CSS is an id selector, not a color.
            let selector = literal.format == ColorFormat::Hex
                && css
                && !line[..i].contains(':')
                && line[literal.range.end..].trim_start().starts_with(['{', ',', '.', ':']);
            if !selector {
                i = literal.range.end;
                out.push(literal);
                continue;
            }
        }
        if boundary && line[i..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            let word_len =
                line[i..].find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(line.len() - i);
            let word = &line[i..i + word_len];
            let after = line[i + word_len..].chars().next();
            if after.is_none_or(|c| !is_word(c) && c != '(')
                && in_property_value(&line[..i])
                && let Some(color) = named::lookup(&word.to_ascii_lowercase())
            {
                out.push(ColorLiteral {
                    range: i..i + word_len,
                    color,
                    format: ColorFormat::Named,
                });
            }
            i += word_len;
            continue;
        }
        i += 1;
    }
    out
}

/// `true` if the preceding text ends in a CSS property value:
/// `color: `, `border: 1px solid `, `style="background:`.
fn in_property_value(before: &str) -> bool {
    // The current declaration starts after the last `;`, `{`, `"`, `'` or `` ` ``.
    let start = before.rfind([';', '{', '"', '\'', '`']).map_or(0, |i| i + 1);
    let declaration = &before[start..];
    let Some(colon) = declaration.find(':') else { return false };
    let property = declaration[..colon].trim();
    let value = &declaration[colon + 1..];
    !property.is_empty()
        && property.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !value.contains(['(', ')', '=', '<', '>'])
}

/// A color (hex, rgb, hsl) that starts exactly at `at`.
fn literal_at(text: &str, at: usize) -> Option<ColorLiteral> {
    let rest = &text[at..];
    if let Some(body) = rest.strip_prefix('#') {
        let digits = body.bytes().take_while(u8::is_ascii_hexdigit).count();
        let next = body[digits..].chars().next();
        if !matches!(digits, 3 | 4 | 6 | 8) || next.is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            return None;
        }
        let hex = &body[..digits];
        let expanded = if digits == 4 {
            // `#rgba` → `#rrggbbaa`.
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            hex.to_owned()
        };
        let color = Rgba::from_hex(&format!("#{expanded}"))?;
        return Some(ColorLiteral { range: at..at + 1 + digits, color, format: ColorFormat::Hex });
    }
    let lower = rest.get(..5).unwrap_or(rest).to_ascii_lowercase();
    let (format, name_len) = if lower.starts_with("rgba(") || lower.starts_with("hsla(") {
        (if lower.starts_with('r') { ColorFormat::Rgb } else { ColorFormat::Hsl }, 5)
    } else if lower.starts_with("rgb(") || lower.starts_with("hsl(") {
        (if lower.starts_with('r') { ColorFormat::Rgb } else { ColorFormat::Hsl }, 4)
    } else {
        return None;
    };
    let close = rest.find(')')?;
    let args = &rest[name_len..close];
    let parts: Vec<&str> =
        args.split([',', ' ', '/']).map(str::trim).filter(|s| !s.is_empty()).collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let alpha = match parts.get(3) {
        Some(a) => number(a, 1.0)?,
        None => 1.0,
    };
    let color = match format {
        ColorFormat::Rgb => Rgba::new(
            number(parts[0], 255.0)? / 255.0,
            number(parts[1], 255.0)? / 255.0,
            number(parts[2], 255.0)? / 255.0,
            alpha,
        ),
        _ => {
            let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?;
            let s = number(parts[1], 1.0)?;
            let l = number(parts[2], 1.0)?;
            Rgba::from_hsl(h, s, l, alpha)
        }
    };
    let clamp = |c: f32| c.clamp(0.0, 1.0);
    let color = Rgba::new(clamp(color.r), clamp(color.g), clamp(color.b), clamp(color.a));
    Some(ColorLiteral { range: at..at + close + 1, color, format })
}

/// A number or a percentage; `scale` = the value of 100%.
fn number(s: &str, scale: f32) -> Option<f32> {
    match s.strip_suffix('%') {
        Some(p) => p.parse::<f32>().ok().map(|v| v / 100.0 * scale),
        None => s.parse::<f32>().ok(),
    }
}

fn alpha_text(a: f32) -> String {
    let s = format!("{:.2}", a.clamp(0.0, 1.0));
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn format_color(color: Rgba, format: ColorFormat) -> String {
    let [r, g, b, a] = color.to_rgba8();
    let opaque = a == 255;
    match format {
        ColorFormat::Named if opaque => named::name_of([r, g, b])
            .map_or_else(|| format!("#{r:02x}{g:02x}{b:02x}"), str::to_owned),
        ColorFormat::Hex | ColorFormat::Named => {
            if opaque {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            }
        }
        ColorFormat::Rgb if opaque => format!("rgb({r}, {g}, {b})"),
        ColorFormat::Rgb => format!("rgba({r}, {g}, {b}, {})", alpha_text(color.a)),
        ColorFormat::Hsl => {
            let (h, s, l) = color.to_hsl();
            let (h, s, l) =
                (h.round() as i32, (s * 100.0).round() as i32, (l * 100.0).round() as i32);
            if opaque {
                format!("hsl({h}, {s}%, {l}%)")
            } else {
                format!("hsla({h}, {s}%, {l}%, {})", alpha_text(color.a))
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
