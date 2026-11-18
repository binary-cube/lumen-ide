//! The blocks laid out whole: code (colored, wrapped, with its language and a copy
//! button over it), tables (columns as wide as their content, aligned as the separator
//! row says) and the front matter (a table of its keys).

use kernel::{Rect, Rgba};
use pulldown_cmark::Alignment;
use text_layout::Font;

use crate::layout::{Align, Builder, Item, Run, Span, Style};
use crate::select::Sep;

pub(crate) struct TableState {
    pub aligns: Vec<Alignment>,
    /// Each row: whether it's the header, and its cells.
    pub rows: Vec<(bool, Vec<Vec<Span>>)>,
}

/// The extension whose language colors a fence's code (`js` for `javascript`).
pub(crate) fn extension(lang: &str) -> &str {
    match lang.to_ascii_lowercase().as_str() {
        "javascript" | "node" | "mjs" | "cjs" => "js",
        "typescript" => "ts",
        "python" | "python3" => "py",
        "ruby" => "rb",
        "rust" => "rs",
        "shell" | "bash" | "zsh" | "console" | "shellsession" | "sh" => "sh",
        "c++" | "cc" | "hpp" | "cxx" => "cpp",
        "yml" => "yaml",
        "jsonc" | "json5" => "json",
        "html5" | "xhtml" | "xml" | "svg" | "vue" => "html",
        "golang" => "go",
        "kotlin" => "kt",
        "csharp" | "c#" => "cs",
        "postgresql" | "postgres" | "mysql" | "sqlite" | "plsql" => "sql",
        "docker" => "dockerfile",
        _ => lang,
    }
}

/// A fence naming no language to color (`text`, `diff` is colored by its lines).
pub(crate) fn plain(lang: &str) -> bool {
    matches!(
        lang.to_ascii_lowercase().as_str(),
        "" | "text" | "txt" | "plain" | "plaintext" | "diff" | "patch" | "mermaid" | "math"
    )
}

/// A code block's line colored by words: its pieces and their colors (`diff`'s lines by
/// their sign).
pub(crate) fn color_line(line: &str, lang: &str, b: &Builder<'_>) -> Vec<(String, Rgba)> {
    let p = b.palette;
    if lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch") {
        let color = match line.chars().next() {
            Some('+') => p.vcs_added,
            Some('-') => p.vcs_deleted,
            Some('@') => p.accent,
            _ => p.code_text,
        };
        return vec![(line.to_owned(), color)];
    }
    if plain(lang) {
        return vec![(line.to_owned(), p.code_text)];
    }
    ui_components::code_tokens::runs(line, extension(lang), p)
        .into_iter()
        .map(|(text, color)| (text.to_owned(), color))
        .collect()
}

/// `body`'s lines colored by its captures (the editor's grammar), the theme's colors.
fn captured_lines(
    body: &str,
    captures: &[(std::ops::Range<usize>, std::sync::Arc<str>)],
    b: &Builder<'_>,
) -> Option<Vec<Vec<(String, Rgba)>>> {
    let syntax = b.palette.syntax.as_ref()?;
    let base = b.palette.code_text;
    let mut colors = vec![base; body.len()];
    for (range, name) in captures {
        if let Some(color) = syntax.capture(name) {
            let end = range.end.min(body.len());
            if range.start < end {
                colors[range.start..end].fill(color);
            }
        }
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for line in body.split('\n') {
        let mut runs: Vec<(String, Rgba)> = Vec::new();
        for (i, c) in line.char_indices() {
            let color = colors[start + i];
            let text = if c == '\t' { "    ".to_owned() } else { c.to_string() };
            match runs.last_mut() {
                Some((run, last)) if *last == color => run.push_str(&text),
                _ => runs.push((text, color)),
            }
        }
        lines.push(runs);
        start += line.len() + 1;
    }
    Some(lines)
}

/// A code block: rounded background, monospace font, colored (by its grammar's
/// `captures` once they're there, by words until then), long lines wrapped, the language
/// in the corner. Kept in the layout's code blocks (their copy button).
pub(crate) fn code_block(
    b: &mut Builder<'_>,
    lang: &str,
    body: &str,
    captures: Option<&[(std::ops::Range<usize>, std::sync::Arc<str>)]>,
    x0: f32,
) {
    let body = body.trim_end_matches('\n');
    let lines = captures.and_then(|c| captured_lines(body, c, b)).unwrap_or_else(|| {
        body.split('\n')
            // Tabs as 4 spaces (the font is monospace).
            .map(|line| color_line(&line.replace('\t', "    "), lang, b))
            .collect()
    });
    code_lines(b, lang, body, &lines, x0);
}

/// [`code_block`] with its lines already colored.
pub(crate) fn code_lines(
    b: &mut Builder<'_>,
    lang: &str,
    body: &str,
    lines: &[Vec<(String, Rgba)>],
    x0: f32,
) {
    let font = Font::new(b.palette.code_font.clone(), (b.palette.code_font_size * b.s).round());
    let metrics = b.text.metrics(&font);
    let line_h = (font.size * 1.5).round();
    let pad = b.px(14.0);
    let room = b.right - x0 - 2.0 * pad;
    // Wrapped (the setting): each line cut into rows that fit (a monospace font: by
    // characters); otherwise a line is a row, and the block scrolls across.
    let wrap = crate::settings::wrap_code();
    let per_row =
        if wrap { ((room / metrics.advance.max(1.0)).floor() as usize).max(8) } else { usize::MAX };
    let mut rows: Vec<Vec<(String, Rgba)>> = Vec::new();
    // Whether each row starts a line of the code (not a wrapped one's rest).
    let mut starts: Vec<bool> = Vec::new();
    for line in lines {
        starts.push(true);
        let mut row: Vec<(String, Rgba)> = Vec::new();
        let mut count = 0;
        for (text, color) in line {
            let mut piece = String::new();
            for c in text.chars() {
                if count == per_row {
                    if !piece.is_empty() {
                        row.push((std::mem::take(&mut piece), *color));
                    }
                    rows.push(std::mem::take(&mut row));
                    starts.push(false);
                    count = 0;
                }
                piece.push(c);
                count += 1;
            }
            if !piece.is_empty() {
                row.push((piece, *color));
            }
        }
        rows.push(row);
    }
    let height = rows.len() as f32 * line_h + 2.0 * pad;
    let rect = Rect::new(x0, b.y, b.right - x0, height);
    let radius = b.px(8.0);
    b.items.push(Item::Rect { rect, color: b.palette.text.with_alpha(0.05), radius });
    let width = b.px(1.0).max(1.0);
    b.items.push(Item::Border { rect, color: b.palette.border, width, radius });
    let label = (!lang.is_empty()).then(|| {
        let small = Font::new(b.palette.ui_font.clone(), b.px(b.palette.ui_font_size - 2.0));
        let w = b.text.shape_line(lang, &small).width;
        let baseline = b.y + b.px(18.0);
        Item::Text {
            x: rect.max_x() - w - b.px(12.0),
            baseline,
            text: lang.to_owned(),
            font: small,
            color: b.palette.text_muted,
        }
    });
    let first =
        b.y + pad + ((line_h - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round();
    let mut texts = Vec::new();
    let mut widest: f32 = 0.0;
    let first_run = b.runs.len();
    for (i, row) in rows.iter().enumerate() {
        let mut x = x0 + pad;
        // Each line of the code a block of the selectable text; a wrapped line's rows go on.
        let mut sep = if i == 0 {
            b.take_sep(Sep::Paragraph)
        } else if starts[i] {
            Sep::Line
        } else {
            Sep::None
        };
        if starts[i] {
            b.blocks += 1;
        }
        let row_top = b.y + pad + i as f32 * line_h;
        if row.is_empty() {
            let rect = Rect::new(x, row_top, 0.0, line_h);
            let empty = std::sync::Arc::new(text_layout::ShapedLine::default());
            b.run(rect, String::new(), Some(empty), String::new(), sep);
        }
        for (text, color) in row {
            let shaped = b.text.shape_line(text, &font);
            let w = shaped.width;
            b.run(Rect::new(x, row_top, w, line_h), text.clone(), Some(shaped), String::new(), sep);
            sep = Sep::None;
            if !text.trim().is_empty() {
                texts.push(Item::Text {
                    x,
                    baseline: first + i as f32 * line_h,
                    text: text.clone(),
                    font: font.clone(),
                    color: *color,
                });
            }
            x += w;
        }
        widest = widest.max(x - x0 - pad);
    }
    if widest > room + 0.5 {
        // Wider than the block: its text scrolls across, clipped inside the frame.
        let view =
            Rect::new(x0 + pad * 0.5, rect.min_y() + 1.0, rect.size.width - pad, height - 2.0);
        let block = b.scrolls;
        b.scrolls += 1;
        for run in &mut b.runs[first_run..] {
            run.scroll = Some(block);
        }
        b.items.push(Item::Scroll {
            view,
            block,
            content: widest + pad,
            room: room + pad,
            items: texts,
        });
        // The language over the scrolled text, on the block's color.
        if let Some(Item::Text { x, baseline, text, font, color }) = label {
            let w = b.text.shape_line(&text, &font).width;
            let pill = Rect::new(x - b.px(6.0), b.y + b.px(4.0), w + b.px(12.0), b.px(20.0));
            let bg = b.palette.editor_background.mix(b.palette.text, 0.05);
            let text = Box::new(Item::Text { x, baseline, text, font, color });
            b.items.push(Item::Badge { pill, bg, text });
        }
    } else {
        b.items.extend(texts);
        b.items.extend(label);
    }
    b.code_blocks.push((rect, body.trim_end_matches('\n').to_owned()));
    b.y += height;
}

/// A table: columns as wide as their content (narrowed to fit, never below their widest
/// word), framed cells, the header on a background, aligned as the separator row says.
pub(crate) fn table(b: &mut Builder<'_>, table: &TableState, x0: f32) {
    let cols = table.rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0).max(table.aligns.len());
    if cols == 0 {
        return;
    }
    let pad = b.px(10.0);
    let available = b.right - x0;
    let mut natural = vec![0.0f32; cols];
    let mut least = vec![0.0f32; cols];
    for (head, row) in &table.rows {
        let weight = if *head { Font::BOLD } else { Font::NORMAL };
        for (c, cell) in row.iter().enumerate() {
            let (w, widest) = b.measure(cell, b.body, weight);
            natural[c] = natural[c].max(w + 2.0 * pad + b.px(2.0));
            least[c] = least[c].max(widest + 2.0 * pad + b.px(2.0));
        }
    }
    let total: f32 = natural.iter().sum();
    let widths: Vec<f32> = if total <= available {
        natural.clone()
    } else {
        let floor: f32 = least.iter().sum();
        if floor >= available {
            least.iter().map(|w| w * available / floor).collect()
        } else {
            let spare = available - floor;
            let wants: f32 = natural.iter().zip(&least).map(|(n, l)| n - l).sum();
            natural
                .iter()
                .zip(&least)
                .map(|(n, l)| l + if wants > 0.0 { (n - l) * spare / wants } else { 0.0 })
                .collect()
        }
    };
    let widths: Vec<f32> = widths.iter().map(|w| w.round()).collect();
    let width: f32 = widths.iter().sum();
    let starts: Vec<f32> = widths
        .iter()
        .scan(x0, |x, w| {
            let at = *x;
            *x += w;
            Some(at)
        })
        .collect();
    let line = b.px(1.0).max(1.0);
    let top = b.y;
    let mut row_tops = Vec::new();
    let first_item = b.items.len();
    for (head, row) in &table.rows {
        let row_top = b.y;
        row_tops.push(row_top);
        let mut bottom = row_top;
        for (c, cell) in row.iter().enumerate() {
            b.y = row_top + pad * 0.6;
            // Copied as rows of cells apart by tabs.
            b.sep = Some(match (c, row_tops.len()) {
                (0, 1) => Sep::Paragraph,
                (0, _) => Sep::Line,
                _ => Sep::Cell,
            });
            let mut cell = cell.clone();
            if *head {
                for span in &mut cell {
                    span.style.bold = true;
                }
            }
            let align = match table.aligns.get(c) {
                Some(Alignment::Center) => Align::Center,
                Some(Alignment::Right) => Align::Right,
                _ => Align::Left,
            };
            let (body, text) = (b.body, b.palette.text);
            b.flow(
                &cell,
                Run {
                    x0: starts[c] + pad,
                    x1: starts[c] + widths[c] - pad,
                    size: body,
                    weight: Font::NORMAL,
                    color: text,
                    line: 1.5,
                    align,
                },
            );
            bottom = bottom.max(b.y + pad * 0.6);
        }
        if row.is_empty() {
            bottom = row_top + b.body * 1.5 + pad * 1.2;
        }
        if *head {
            b.items.insert(
                first_item,
                Item::Rect {
                    rect: Rect::new(x0, row_top, width, bottom - row_top),
                    color: b.palette.text.with_alpha(0.05),
                    radius: 0.0,
                },
            );
        }
        b.y = bottom;
    }
    let rect = Rect::new(x0, top, width, b.y - top);
    for row_top in row_tops.iter().skip(1) {
        b.items.push(Item::Rect {
            rect: Rect::new(x0, *row_top, width, line),
            color: b.palette.border,
            radius: 0.0,
        });
    }
    for x in starts.iter().skip(1) {
        b.items.push(Item::Rect {
            rect: Rect::new(x.round(), top, line, rect.size.height),
            color: b.palette.border,
            radius: 0.0,
        });
    }
    let radius = line * 4.0;
    b.items.push(Item::Border { rect, color: b.palette.border, width: line, radius });
}

/// The front matter (`---` YAML / `+++` TOML at the top) as a table of its top-level keys
/// and values, as GitHub shows it; `false` if it has none.
pub(crate) fn front_matter(b: &mut Builder<'_>, text: &str, x0: f32) -> bool {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let top = !line.starts_with([' ', '\t', '-']);
        let split = line.split_once(':').or_else(|| line.split_once('='));
        match split {
            Some((key, value)) if top => {
                let value = value.trim().trim_matches('"').trim_matches('\'');
                pairs.push((key.trim().to_owned(), value.to_owned()));
            }
            _ => {
                if let Some((_, value)) = pairs.last_mut() {
                    let item = line.trim().trim_start_matches('-').trim();
                    if !value.is_empty() {
                        value.push_str(", ");
                    }
                    value.push_str(item);
                }
            }
        }
    }
    if pairs.is_empty() {
        return false;
    }
    let bold = Style { bold: true, ..Style::default() };
    let rows = pairs
        .into_iter()
        .map(|(k, v)| {
            (
                false,
                vec![vec![Span::text(k, bold, None)], vec![Span::text(v, Style::default(), None)]],
            )
        })
        .collect();
    table(b, &TableState { aligns: vec![Alignment::Left, Alignment::Left], rows }, x0);
    true
}
