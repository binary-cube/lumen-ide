//! Text that fits: ellipsized (at the end, the start, the middle), wrapped, a section's title.

use super::*;
use text_layout::ShapedLine;

/// Where `value`'s glyphs (as `shaped`) stop ending within `room` from its start: the byte
/// its text is cut at (a character's start).
fn head_end(shaped: &ShapedLine, value: &str, room: f32) -> usize {
    let at = shaped
        .glyphs
        .iter()
        .find(|g| g.x + g.advance > room + 0.01)
        .map_or(value.len(), |g| g.start);
    value.floor_char_boundary(at.min(value.len()))
}

/// Where the glyphs of `value` (as `shaped`) that fit `room` at its end begin.
fn tail_start(shaped: &ShapedLine, value: &str, room: f32) -> usize {
    let from = shaped.width - room;
    let at = shaped.glyphs.iter().find(|g| g.x >= from - 0.01).map_or(value.len(), |g| g.start);
    value.ceil_char_boundary(at.min(value.len()))
}

/// The cut text, made smaller one character at a time while it's still wider than `max`
/// (a glyph's place in the whole text and in the cut one can differ a little: kerning, a
/// ligature broken): usually it fits at once.
fn fitted(
    text: &mut TextSystem,
    font: &Font,
    max: f32,
    mut keep: usize,
    make: impl Fn(usize) -> String,
    smaller: impl Fn(usize) -> usize,
) -> String {
    loop {
        let out = make(keep);
        if keep == 0 || text.shape_line(&out, font).width <= max {
            return out;
        }
        keep = smaller(keep);
    }
}

/// The byte before `at` in `s` (a character back).
fn back(s: &str, at: usize) -> usize {
    s[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

/// The text shortened with "…" to fit in `max` pixels (given font); whole if it fits. Cut
/// where its glyphs (shaped once, kept by the text system) stop fitting: no text is shaped
/// again for each try.
pub fn ellipsize(text: &mut TextSystem, value: &str, font: &Font, max: f32) -> String {
    let shaped = text.shape_line(value, font);
    if shaped.width <= max {
        return value.to_owned();
    }
    let dots = text.shape_line("…", font).width;
    let cut = head_end(&shaped, value, (max - dots).max(0.0));
    fitted(
        text,
        font,
        max,
        cut,
        |cut| value[..cut].trim_end().to_owned() + "…",
        |cut| back(value, cut),
    )
}

/// `value` cut at its start with `…` to fit `max` (the end stays: what comes right
/// before something shown after it); whole if it fits.
pub fn ellipsize_start(text: &mut TextSystem, value: &str, font: &Font, max: f32) -> String {
    let shaped = text.shape_line(value, font);
    if shaped.width <= max {
        return value.to_owned();
    }
    let dots = text.shape_line("…", font).width;
    let from = tail_start(&shaped, value, (max - dots).max(0.0));
    // `keep` counts the bytes kept at the end.
    fitted(
        text,
        font,
        max,
        value.len() - from,
        |keep| format!("…{}", &value[value.len() - keep..]),
        |keep| {
            let at = value.len() - keep;
            let next = value[at..].chars().next().map_or(0, char::len_utf8);
            keep.saturating_sub(next)
        },
    )
}

/// The text shortened in the middle with "…" (both ends stay: the start of a path and its
/// file name); whole if it fits.
pub fn ellipsize_middle(text: &mut TextSystem, value: &str, font: &Font, max: f32) -> String {
    let shaped = text.shape_line(value, font);
    if shaped.width <= max {
        return value.to_owned();
    }
    let dots = text.shape_line("…", font).width;
    let room = (max - dots).max(0.0);
    // Half the room for the start (its larger half), the rest for the end.
    let head = head_end(&shaped, value, room / 2.0);
    let head_width = shaped.x_at(head);
    let tail = tail_start(&shaped, value, room - head_width).max(head);
    // Made smaller by turns while too wide: the end, then the start.
    let (mut head, mut tail) = (head, tail);
    let mut turn = 0usize;
    loop {
        let out = format!("{}…{}", &value[..head], &value[tail..]);
        if (head == 0 && tail == value.len()) || text.shape_line(&out, font).width <= max {
            return out;
        }
        turn += 1;
        let next = value[tail..].chars().next().map_or(0, char::len_utf8);
        if (turn % 2 == 1 || head == 0) && next > 0 {
            tail += next;
        } else {
            head = back(value, head);
        }
    }
}

/// The widest a file's name is shown where it's one of several side by side (a tab, a
/// breadcrumb), logical pixels: a longer one is cut by [`ellipsize_file`], whole in its
/// tooltip.
pub const NAME_MAX: f32 = 220.0;

/// The widest a file's path is shown on a line it shares (the status bar), logical pixels:
/// a longer one is cut by [`ellipsize_path`], whole in its tooltip.
pub const PATH_MAX: f32 = 420.0;

/// A file's name shortened to fit `max`: its extension and the end of its stem always kept
/// (`asdfasdf asdf asdfa…fsad.php` — the start of the name, then what it is), the rest of the
/// stem from its start; whole if it fits. A name without an extension (or too short to keep
/// one): [`ellipsize_middle`]. A path's last part is the name (the folders: cut first).
pub fn ellipsize_file(text: &mut TextSystem, name: &str, font: &Font, max: f32) -> String {
    let shaped = text.shape_line(name, font);
    if shaped.width <= max {
        return name.to_owned();
    }
    // The extension: after the last dot, not a leading one (`.env`), short (`.php`, `.blade.php`'s
    // `.php`), never the whole name.
    let ext_at = name.rfind('.').filter(|&i| i > 0 && name.len() - i <= 12);
    let Some(ext_at) = ext_at else { return ellipsize_middle(text, name, font, max) };
    let stem = &name[..ext_at];
    // The stem's end kept beside the extension: a few characters (what tells similar names
    // apart: `…_v2.php`).
    let tail_at = stem.char_indices().rev().nth(3).map_or(0, |(i, _)| i);
    let suffix = format!("…{}", &name[tail_at..]);
    let suffix_width = text.shape_line(&suffix, font).width;
    // Not even the extension with one letter fits: the plain middle cut.
    if suffix_width > max {
        return ellipsize_middle(text, name, font, max);
    }
    let head = head_end(&shaped, name, max - suffix_width).min(tail_at);
    fitted(
        text,
        font,
        max,
        head,
        |head| format!("{}{suffix}", &name[..head]),
        |head| back(name, head),
    )
}

/// A path (`src/Http/Controllers/UserController.php`) shortened to fit `max`: its file's name
/// first (whole, or [`ellipsize_file`] when even it doesn't fit), then as much of the
/// folders as fits from their start, the rest `…` (`src/…/UserController.php`).
pub fn ellipsize_path(text: &mut TextSystem, path: &str, font: &Font, max: f32) -> String {
    let shaped = text.shape_line(path, font);
    if shaped.width <= max {
        return path.to_owned();
    }
    let Some((dirs, name)) = path.rsplit_once('/') else {
        return ellipsize_file(text, path, font, max);
    };
    let tail = format!("…/{name}");
    let tail_width = text.shape_line(&tail, font).width;
    if tail_width > max {
        return ellipsize_file(text, name, font, max);
    }
    let head = head_end(&shaped, path, max - tail_width).min(dirs.len());
    fitted(
        text,
        font,
        max,
        head,
        |head| format!("{}{tail}", &path[..head]),
        |head| back(path, head),
    )
}

/// `value` in lines no wider than `max`, broken between words (a word longer than a line is
/// cut short); at most `lines` of them, the last ending in `…` when there's more.
pub fn wrap(
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    max: f32,
    lines: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut words = value.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let next = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if line.is_empty() || text.shape_line(&next, font).width <= max {
            line = next;
            continue;
        }
        if out.len() + 1 == lines {
            // The last line: the rest after it, cut short.
            let rest: Vec<&str> = std::iter::once(word).chain(words).collect();
            line = format!("{line} {}", rest.join(" "));
            break;
        }
        out.push(std::mem::replace(&mut line, word.to_owned()));
    }
    if !line.is_empty() {
        out.push(line);
    }
    out.truncate(lines.max(1));
    out.into_iter().map(|l| ellipsize(text, &l, font, max)).collect()
}

/// `value` drawn from `x` on `baseline`, cut with "…" where it passes `max` (its whole
/// width otherwise): as [`ellipsize`], but nothing allocated — the shaped line's glyphs that
/// fit, then the "…" (both shaped once, kept by the text system), so a view can draw a long
/// name every frame. Returns the width drawn and whether it was cut.
pub fn draw_ellipsized(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    (x, baseline): (f32, f32),
    max: f32,
    color: Rgba,
) -> (f32, bool) {
    let line = text.shape_line(value, font);
    if line.width <= max + 0.5 {
        draw_text(layer, &line, x, baseline, color);
        return (line.width, false);
    }
    let dots = text.shape_line("…", font);
    let room = max - dots.width;
    if room <= 0.0 {
        return (0.0, true);
    }
    // The glyphs that end before the room does (a trailing space left out).
    let mut end = 0.0_f32;
    let mut count = 0;
    for (i, g) in line.glyphs.iter().enumerate() {
        if g.x + g.advance > room {
            break;
        }
        let blank = value.as_bytes().get(g.start).is_some_and(u8::is_ascii_whitespace);
        if !blank {
            end = end.max(g.x + g.advance);
            count = i + 1;
        }
    }
    layer.glyphs.extend(line.glyphs[..count].iter().map(|g| render_api::GlyphInstance {
        key: g.key,
        origin: kernel::Point::new(x + g.x, baseline + g.y),
        color,
    }));
    draw_text(layer, &dots, x + end, baseline, color);
    (end + dots.width, true)
}

/// A content section title (and an optional description); returns the y below it.
pub fn section_title(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    x: f32,
    y: f32,
    title: &str,
    detail: Option<&str>,
) -> f32 {
    section_title_in(layer, text, ctx, (x, y, f32::INFINITY), title, detail)
}

/// The font of a [`section_title`].
pub fn section_title_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(0.5).with_weight(Font::SEMIBOLD)
}

/// [`section_title`] kept within `width` from `x`: a title or a description longer than
/// that (one made from what the user typed) is cut with "…", never drawn past its room.
pub fn section_title_in(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (x, y, width): (f32, f32, f32),
    title: &str,
    detail: Option<&str>,
) -> f32 {
    let font = section_title_font(ctx);
    let m = text.metrics(&font);
    let baseline = (y + m.ascent).round();
    draw_ellipsized(layer, text, title, &font, (x, baseline), width, ctx.fade(ctx.palette.text));
    let mut bottom = y + m.ascent + m.descent;
    if let Some(detail) = detail {
        let small = ctx.ui_font(-1.0);
        let sm = text.metrics(&small);
        draw_ellipsized(
            layer,
            text,
            detail,
            &small,
            (x, (bottom + ctx.px(3.0) + sm.ascent).round()),
            width,
            ctx.fade(ctx.palette.text_muted),
        );
        bottom += ctx.px(3.0) + sm.ascent + sm.descent;
    }
    bottom
}

/// How tall a [`section_header`] is (logical pixels).
pub const SECTION_HEADER: f32 = 22.0;
/// The space above a section after the first and below one before the last (logical
/// pixels): between two sections, twice this; the first and the last keep only their
/// container's padding.
pub const SECTION_SPACE: f32 = 14.0;

/// A section's header in a long panel (a form's parts): its icon, its title in capitals and
/// a thin line filling the rest of `width` — "[icon] TITLE ─────". Returns the y below it
/// (where the section's content starts).
pub fn section_header(
    pen: &mut LayerPainter<'_>,
    (x, y, width): (f32, f32, f32),
    icon: &str,
    title: &str,
) -> f32 {
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ctx.palette;
    let h = ctx.px(SECTION_HEADER);
    let size = ctx.px(14.0).round();
    let muted = ctx.fade(p.text_muted);
    let mut tx = x;
    if let Some(id) = icons.id(icon) {
        let r = Rect::new(x, (y + (h - size) / 2.0).round(), size, size);
        ui_kit::draw_icon(layer, icons, id, r, muted);
        tx = x + size + ctx.px(8.0);
    }
    let font = ctx.ui_font(-2.5).with_weight(Font::SEMIBOLD);
    let shaped = text.shape_line(&title.to_uppercase(), &font);
    draw_text(layer, &shaped, tx, optical_baseline(y, h, &font), muted);
    let line_x = (tx + shaped.width + ctx.px(10.0)).round();
    let end = x + width;
    if end > line_x {
        let line_y = (y + h / 2.0).round();
        layer.quad(Quad::filled(
            Rect::new(line_x, line_y, end - line_x, ctx.line()),
            ctx.fade(p.border),
        ));
    }
    y + h + ctx.px(10.0)
}

/// A name's initials for an avatar: the first letters of its first two words
/// (`docker.acme.test` → "DA", `www-data` → "WD"), otherwise its first two letters.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> =
        name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let initials: String = match words.as_slice() {
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
        [only] => only.chars().take(2).collect(),
        [] => "?".into(),
    };
    initials.to_uppercase()
}
