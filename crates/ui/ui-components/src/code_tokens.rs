//! Coloring of code rows outside the editor (canvas, diff): a small lexical tokenizer
//! (keywords, strings, variables, numbers, comments), with the theme's ANSI colors (adapts
//! to the light / dark theme), no parser. What's a keyword, a variable or a comment is its
//! language's ([`code_intelligence::code_style`], by the file's extension).

use crate::palette::Palette;
use kernel::Rgba;
use render_api::Layer;
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

/// `word` is a keyword (or a built-in type) of the language of files with `extension`.
pub fn is_keyword(extension: &str, word: &str) -> bool {
    code_intelligence::code_style::for_extension(extension).is_keyword(word)
}

/// What [`draw_code`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct CodeLine<'a> {
    pub source: &'a str,
    /// The extension of the file it's from (its language colors it).
    pub extension: &'a str,
    pub font: &'a Font,
    pub x: f32,
    pub baseline: f32,
    pub palette: &'a Palette,
    pub max_x: f32,
}

/// Draws `source` from `x`, colored, clipped at `max_x`.
pub fn draw_code(layer: &mut Layer, text: &mut TextSystem, spec: CodeLine<'_>) {
    let CodeLine { source, extension, font, x, baseline, palette, max_x } = spec;
    let style = code_intelligence::code_style::for_extension(extension);
    let mut x = x;
    for (token, color) in tokens(source, palette, &style) {
        if x >= max_x {
            break;
        }
        let shaped = text.shape_line(token, font);
        draw_text(layer, &shaped, x, baseline, color);
        x += shaped.width;
    }
}

/// Draws `source` as [`draw_code`] does, but a line too long for `max_x` ends in "…" (in the
/// color of the piece it cuts) instead of a glyph cut in half: for one-line listings.
pub fn draw_code_fitted(layer: &mut Layer, text: &mut TextSystem, spec: CodeLine<'_>) {
    let CodeLine { source, extension, font, x, baseline, palette, max_x } = spec;
    let style = code_intelligence::code_style::for_extension(extension);
    let pieces: Vec<_> = tokens(source, palette, &style)
        .into_iter()
        .map(|(token, color)| (text.shape_line(token, font), color))
        .collect();
    let total: f32 = pieces.iter().map(|(s, _)| s.width).sum();
    let mut x = x;
    if x + total <= max_x {
        for (shaped, color) in &pieces {
            draw_text(layer, shaped, x, baseline, *color);
            x += shaped.width;
        }
        return;
    }
    let dots = text.shape_line("…", font);
    let limit = max_x - dots.width;
    for (shaped, color) in pieces {
        if x + shaped.width <= limit {
            draw_text(layer, &shaped, x, baseline, color);
            x += shaped.width;
            continue;
        }
        // The part of this piece that fits, then the dots.
        let mut cut = (*shaped).clone();
        cut.glyphs.retain(|g| x + g.x + g.advance <= limit);
        let end = cut.glyphs.last().map_or(0.0, |g| g.x + g.advance);
        cut.width = end;
        draw_text(layer, &cut, x, baseline, color);
        draw_text(layer, &dots, x + end, baseline, color);
        return;
    }
}

/// `source`'s pieces in order (covering it whole) with their colors, as [`draw_code`]
/// draws them: for a caller drawing a part of a line (a wrapped row) itself.
pub fn runs<'a>(source: &'a str, extension: &str, palette: &Palette) -> Vec<(&'a str, Rgba)> {
    tokens(source, palette, &code_intelligence::code_style::for_extension(extension))
}

fn tokens<'a>(
    source: &'a str,
    p: &Palette,
    style: &code_intelligence::code_style::CodeStyle,
) -> Vec<(&'a str, Rgba)> {
    // The theme's syntax colors (the terminal's, before a theme is applied).
    let ansi = &p.terminal_ansi;
    let (keyword, string, variable, number, comment, call) = match &p.syntax {
        Some(s) => (s.keyword, s.string, s.variable, s.number, s.comment, s.function),
        None => (ansi[5], ansi[2], ansi[1], ansi[4], p.text_muted, ansi[6]),
    };
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    let trimmed = source.trim_start();
    if style.comments.iter().any(|c| trimmed.starts_with(c)) {
        return vec![(source, comment)];
    }
    while i < source.len() {
        let c = bytes[i];
        let start = i;
        let color;
        if c == b'\'' || c == b'"' || c == b'`' {
            i += 1;
            while i < source.len() && bytes[i] != c {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(source.len());
            color = string;
        } else if style.sigil.is_some_and(|s| source[i..].starts_with(s)) {
            i += style.sigil.map_or(1, char::len_utf8);
            while i < source.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            color = variable;
        } else if c.is_ascii_digit() {
            while i < source.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            color = number;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < source.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &source[start..i];
            color = if style.is_keyword(word) {
                keyword
            } else if source[i..].starts_with('(') {
                call
            } else {
                p.code_text
            };
        } else if style.comments.first().is_some_and(|c| source[i..].starts_with(c)) {
            i = source.len();
            color = comment;
        } else {
            // The rest (operators, spaces, non-ASCII characters): up to the next token.
            i += source[i..].chars().next().map_or(1, char::len_utf8);
            while i < source.len()
                && !matches!(bytes[i], b'\'' | b'"' | b'`' | b'$' | b'/')
                && !bytes[i].is_ascii_alphanumeric()
                && bytes[i] != b'_'
            {
                i += source[i..].chars().next().map_or(1, char::len_utf8);
            }
            color = p.code_text;
        }
        out.push((&source[start..i.min(source.len())], color));
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/code_tokens.rs"]
mod tests;
