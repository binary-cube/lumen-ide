//! The grid drawn: each cell's colors (the theme's or the program's own), its glyphs, the
//! selection and the cursor.

use super::*;

/// Color of a cell: defaults and the 16 ANSI colors as chosen, the 6×6×6 cube and grays
/// for 16–255; colors redefined by programs (OSC 4 / 10 / 11) take priority.
pub(super) fn resolve(
    color: TermColor,
    foreground: bool,
    grid: &GridSnapshot,
    colors: &TermColors,
) -> Rgba {
    let rgb = |(r, g, b): (u8, u8, u8)| {
        Rgba::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    };
    match color {
        TermColor::Default => {
            let index = if foreground { 256 } else { 257 };
            match grid.palette_overrides.get(&index) {
                Some(&c) => rgb(c),
                None if foreground => colors.foreground,
                None => colors.background,
            }
        }
        TermColor::Rgb(r, g, b) => rgb((r, g, b)),
        TermColor::Indexed(i) => {
            if let Some(&c) = grid.palette_overrides.get(&u16::from(i)) {
                return rgb(c);
            }
            match i {
                0..=15 => colors.ansi[i as usize],
                _ => rgb(terminal_session::indexed_rgb(i)),
            }
        }
    }
}

/// A cell's foreground and background as drawn: bold in the bright colors when chosen (the
/// first 8 become their bright ones), inverse swapped, dim faded.
pub(super) fn cell_colors(
    c: &terminal_session::TermCell,
    grid: &GridSnapshot,
    colors: &TermColors,
    bold_bright: bool,
) -> (Rgba, Rgba) {
    let fg = match c.fg {
        TermColor::Indexed(i) if bold_bright && i < 8 && c.flags & flags::BOLD != 0 => {
            TermColor::Indexed(i + 8)
        }
        other => other,
    };
    let mut fg = resolve(fg, true, grid, colors);
    let mut bg = resolve(c.bg, false, grid, colors);
    if c.flags & flags::INVERSE != 0 {
        std::mem::swap(&mut fg, &mut bg);
    }
    if c.flags & flags::DIM != 0 {
        fg = fg.with_alpha(fg.a * 0.6);
    }
    (fg, bg)
}

/// How the grid is drawn: the cell's size, the font, the colors and the cursor.
pub(super) struct GridStyle<'a> {
    pub(super) cell: (f32, f32),
    pub(super) font: &'a Font,
    /// The view has the keys (a solid cursor; else its outline).
    pub(super) focused: bool,
    /// The cursor blinks (as chosen in the settings), and which one it is (its glide and
    /// trail: `ui_kit::caret`).
    pub(super) blink: bool,
    pub(super) caret_id: u64,
    pub(super) colors: &'a TermColors,
    /// Bold text in the bright colors (the first 8 become their bright ones).
    pub(super) bold_bright: bool,
    pub(super) scale: f32,
}

pub(super) fn paint_grid(
    layer: &mut Layer,
    grid: &GridSnapshot,
    area: Rect,
    text: &mut TextSystem,
    style: &GridStyle<'_>,
) {
    let GridStyle { cell: (cell_w, cell_h), font, focused, colors: palette, .. } = *style;
    // Its four faces: regular, bold, italic, bold italic.
    let faces = [
        font.clone(),
        font.clone().with_weight(Font::BOLD),
        font.clone().with_italic(true),
        font.clone().with_weight(Font::BOLD).with_italic(true),
    ];
    let face = |flags: u8| {
        let bold = usize::from(flags & flags::BOLD != 0);
        let italic = usize::from(flags & flags::ITALIC != 0);
        &faces[bold + 2 * italic]
    };
    let metrics = text.metrics(font);
    let default_bg = resolve(TermColor::Default, false, grid, palette);
    let thin = style.scale.round().max(1.0);
    let colors = |c: &terminal_session::TermCell| cell_colors(c, grid, palette, style.bold_bright);
    // The bits that choose the face.
    let face_bits = |c: &terminal_session::TermCell| c.flags & (flags::BOLD | flags::ITALIC);

    let mut run = String::new();
    // The history line above the screen first, while it's scrolled by part of a line (the
    // grid shifted down: `area` starts lower, the layer's clip cuts what's outside).
    let first: i32 = if grid.above.is_empty() { 0 } else { -1 };
    for row in first..i32::from(grid.rows) {
        let cells = if row < 0 { &grid.above[..] } else { grid.row(row as u16) };
        let top = area.min_y() + row as f32 * cell_h;
        let baseline = centered_baseline(top, cell_h, &metrics);
        let x_of = |col: usize| (area.min_x() + col as f32 * cell_w).round();

        // Backgrounds, merged into runs of the same color.
        let mut col = 0;
        while col < cells.len() {
            let (_, bg) = colors(&cells[col]);
            let start = col;
            while col < cells.len() && colors(&cells[col]).1 == bg {
                col += 1;
            }
            if bg != default_bg {
                layer.quad(Quad::filled(
                    Rect::new(x_of(start), top, x_of(col) - x_of(start), cell_h),
                    bg,
                ));
            }
        }

        // Text, in ASCII runs of the same style (aligned exactly to cells); the remaining
        // characters (box-drawing, emoji, CJK) one at a time, at their cell.
        let mut col = 0;
        while col < cells.len() {
            let cell = cells[col];
            let (fg, _) = colors(&cell);
            if cell.flags & flags::HIDDEN != 0 || cell.ch == ' ' {
                col += 1;
                continue;
            }
            let bits = face_bits(&cell);
            let f = face(cell.flags);
            if !cell.ch.is_ascii() {
                let shaped = text.shape_line(cell.ch.encode_utf8(&mut [0; 4]), f);
                draw_text(layer, &shaped, x_of(col), baseline, fg);
                col += 1;
                continue;
            }
            let start = col;
            run.clear();
            while col < cells.len() {
                let c = cells[col];
                let same = c.ch.is_ascii()
                    && c.flags & flags::HIDDEN == 0
                    && face_bits(&c) == bits
                    && colors(&c).0 == fg;
                if !same {
                    break;
                }
                run.push(c.ch);
                col += 1;
            }
            let trimmed = run.trim_end();
            let shaped = text.shape_line(trimmed, f);
            draw_text(layer, &shaped, x_of(start), baseline, fg);
            for (i, c) in cells[start..col].iter().enumerate() {
                if c.flags & (flags::UNDERLINE | flags::STRIKE) != 0 {
                    let y = if c.flags & flags::UNDERLINE != 0 {
                        baseline + thin * 2.0
                    } else {
                        top + cell_h / 2.0
                    };
                    layer.quad(Quad::filled(
                        Rect::new(x_of(start + i), y.round(), cell_w.ceil(), thin),
                        fg,
                    ));
                }
            }
        }
    }

    // Cursor: drawn as the caret's look says when focused (blinking as chosen here, its glow,
    // glide and trail), an outline when not; its shape the program's, else the settings'.
    let Some((row, col)) = grid.cursor else { return };
    if grid.cursor_shape == CursorShape::Hidden || grid.display_offset > 0 {
        return;
    }
    let x = (area.min_x() + f32::from(col) * cell_w).round();
    let y = (area.min_y() + f32::from(row) * cell_h).round();
    let rect = Rect::new(x, y, cell_w.round(), cell_h);
    let cursor = palette.cursor;
    if !focused {
        layer.quad(Quad::filled(rect, Rgba::TRANSPARENT).bordered(thin, cursor));
        return;
    }
    let shape = match grid.cursor_shape {
        CursorShape::Beam => CaretShape::Bar,
        CursorShape::Underline => CaretShape::Underline,
        _ => CaretShape::Block,
    };
    let drawn = ui_kit::caret::paint(
        layer,
        ui_kit::caret::CaretPaint {
            id: style.caret_id,
            cell: rect,
            color: cursor,
            scale: style.scale,
            shape: Some(shape),
            thickness: Some((2.0 * style.scale).round()),
            steady: !style.blink,
        },
    );
    // A block shows its character over it (while it's mostly shown).
    if shape == CaretShape::Block && drawn.is_some() && (!style.blink || ui_kit::caret::visible()) {
        let cell = grid.row(row).get(col as usize).copied().unwrap_or_default();
        if cell.ch != ' ' {
            let shaped = text.shape_line(cell.ch.encode_utf8(&mut [0; 4]), face(cell.flags));
            let baseline = centered_baseline(y, cell_h, &metrics);
            // Glyphs are drawn over quads in the same layer.
            draw_text(layer, &shaped, x, baseline, default_bg);
        }
    }
}
