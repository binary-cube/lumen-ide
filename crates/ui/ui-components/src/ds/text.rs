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
/// cut short); at most `lines` of them, the last ending in `…` when there's more. Its words
/// (joined by single spaces) are shaped once and broken where their glyphs pass `max`; a
/// line known to fit as it was filled isn't measured again, only one that may not (a word
/// alone on its line, the rest after the last) is, and cut when it doesn't.
pub fn wrap(
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    max: f32,
    lines: usize,
) -> Vec<String> {
    let lines = lines.max(1);
    let words = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        return Vec::new();
    }
    let shaped = text.shape_line(&words, font);
    let fits = |text: &mut TextSystem, (from, to): (usize, usize)| {
        let measured = shaped.x_at(to) - shaped.x_at(from);
        match (measured - max).abs() <= SNAP_SLACK {
            // Too close to tell from the snapped positions: the run shaped alone.
            true => text.shape_line(&words[from..to], font).width <= max,
            false => measured <= max,
        }
    };
    // Each line: where it starts and ends, and whether it was measured to fit.
    let mut broken: Vec<(usize, usize, bool)> = Vec::new();
    // The line being filled: where it starts, where its next word starts, and whether it
    // was measured to fit as far as it goes (its first word alone isn't).
    let (mut start, mut word_start, mut fit) = (0, 0, false);
    for (space, _) in words.match_indices(' ').chain(std::iter::once((words.len(), ""))) {
        // A word always goes on an empty line; another one while the line still fits.
        if word_start == start {
            word_start = space + 1;
            continue;
        }
        if fits(text, (start, space)) {
            (word_start, fit) = (space + 1, true);
            continue;
        }
        if broken.len() + 1 == lines {
            // The last line: the rest after it, measured again to be cut short.
            fit = false;
            break;
        }
        broken.push((start, word_start - 1, fit));
        // The word that didn't fit starts the next line.
        (start, word_start, fit) = (word_start, space + 1, false);
    }
    broken.push((start, words.len(), fit));
    broken
        .into_iter()
        .map(|(from, to, fit)| match fit || fits(text, (from, to)) {
            true => words[from..to].to_owned(),
            false => ellipsize(text, &words[from..to], font, max),
        })
        .collect()
}

/// How far a run measured inside its line may be from its width alone (physical pixels):
/// the glyphs' places are snapped to whole pixels.
const SNAP_SLACK: f32 = 1.0;

/// `value` drawn from `x` on `baseline`, cut with "…" where it passes `max` (its whole
/// width otherwise): as [`ellipsize`], but nothing allocated — the shaped line's glyphs that
/// fit, then the "…" (both shaped once, kept by the text system), so a view can draw a long
/// name every frame. Returns the width drawn and whether it was cut.
pub fn draw_ellipsized(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    at: (f32, f32),
    max: f32,
    color: Rgba,
) -> (f32, bool) {
    draw_ellipsized_marked(layer, text, value, font, at, max, (color, Marks::NONE))
}

/// Characters of a text drawn in a color of their own (what a search matched): their byte
/// positions in it, in order.
#[derive(Debug, Clone, Copy)]
pub struct Marks<'a> {
    pub at: &'a [usize],
    pub color: Rgba,
    /// Where the text drawn starts in the one `at` counts in (a path's name: after its
    /// folder).
    pub from: usize,
}

impl Marks<'_> {
    pub const NONE: Marks<'static> = Marks { at: &[], color: Rgba::TRANSPARENT, from: 0 };

    /// The color of the glyph starting at `byte`.
    fn color_at(&self, byte: usize, color: Rgba) -> Rgba {
        if self.at.binary_search(&(self.from + byte)).is_ok() { self.color } else { color }
    }
}

/// [`draw_ellipsized`] with `marks`' characters in their own color (those cut off left out).
pub fn draw_ellipsized_marked(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    (x, baseline): (f32, f32),
    max: f32,
    (color, marks): (Rgba, Marks<'_>),
) -> (f32, bool) {
    let line = text.shape_line(value, font);
    let fits = line.width <= max + 0.5;
    if fits && marks.at.is_empty() {
        draw_text(layer, &line, x, baseline, color);
        return (line.width, false);
    }
    let dots = text.shape_line("…", font);
    let (count, end) = if fits {
        (line.glyphs.len(), line.width)
    } else {
        let Some(cut) = cut(&line, value, max - dots.width) else { return (0.0, true) };
        cut
    };
    let shown = if fits { end } else { end + dots.width };
    // To a screen reader: the whole value, where it shows.
    ui_kit::access::text_shown(&line, (x, shown), baseline);
    layer.glyphs.extend(line.glyphs[..count].iter().map(|g| render_api::GlyphInstance {
        key: g.key,
        origin: kernel::Point::new(x + g.x, baseline + g.y),
        color: marks.color_at(g.start, color),
    }));
    if !fits {
        draw_text(layer, &dots, x + end, baseline, color);
    }
    (shown, !fits)
}

/// `value` drawn from `x` on `baseline`, cut in the middle with "…" where it passes `max`
/// (its start and its end kept: a path's root and its last folders), nothing allocated:
/// the shaped line's first glyphs, the "…", its last glyphs. Returns the width drawn.
pub fn draw_ellipsized_middle(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    (x, baseline): (f32, f32),
    max: f32,
    color: Rgba,
) -> f32 {
    let line = text.shape_line(value, font);
    if line.width <= max + 0.5 {
        draw_text(layer, &line, x, baseline, color);
        return line.width;
    }
    let dots = text.shape_line("…", font);
    let room = max - dots.width;
    if room <= 0.0 {
        return 0.0;
    }
    // Half the room for the start, the rest for the end.
    let (head, head_w) = cut(&line, value, room / 2.0).unwrap_or((0, 0.0));
    let tail_from = line
        .glyphs
        .iter()
        .skip(head)
        .find(|g| line.width - g.x <= room - head_w + 0.01)
        .map_or(line.width, |g| g.x);
    let shown = head_w + dots.width + (line.width - tail_from);
    ui_kit::access::text_shown(&line, (x, shown), baseline);
    let glyph = |g: &text_layout::ShapedGlyph, at: f32| render_api::GlyphInstance {
        key: g.key,
        origin: kernel::Point::new(at, baseline + g.y),
        color,
    };
    layer.glyphs.extend(line.glyphs[..head].iter().map(|g| glyph(g, x + g.x)));
    draw_text(layer, &dots, x + head_w, baseline, color);
    let shift = x + head_w + dots.width - tail_from;
    layer.glyphs.extend(
        line.glyphs.iter().skip(head).filter(|g| g.x >= tail_from).map(|g| glyph(g, shift + g.x)),
    );
    shown
}

/// The width [`draw_ellipsized`] would draw `value` in (its whole width, or cut with "…"
/// to `max`), nothing drawn and nothing allocated: a text placed by its right edge.
pub fn ellipsized_width(text: &mut TextSystem, value: &str, font: &Font, max: f32) -> f32 {
    let line = text.shape_line(value, font);
    if line.width <= max + 0.5 {
        return line.width;
    }
    let dots = text.shape_line("…", font).width;
    cut(&line, value, max - dots).map_or(0.0, |(_, end)| end + dots)
}

/// Where a line is cut to leave `room`: how many of its glyphs, and where the last ends (a
/// trailing space left out). `None`: no room at all.
fn cut(line: &text_layout::ShapedLine, value: &str, room: f32) -> Option<(usize, f32)> {
    if room <= 0.0 {
        return None;
    }
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
    Some((count, end))
}

/// How tall a section title's heading is told to a screen reader (logical pixels).
const SECTION_TITLE_REACH: f32 = 24.0;

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
    place: (f32, f32, f32),
    title: &str,
    detail: Option<&str>,
) -> f32 {
    section_title_lines(layer, text, ctx, place, SectionTitle { title, detail, lines: &[] })
}

/// A section's title and its description, as [`section_title_lines`] draws them.
#[derive(Debug, Clone, Copy, Default)]
pub struct SectionTitle<'a> {
    pub title: &'a str,
    /// Its description on one line, cut with "…" where it passes the width (`None`: none).
    pub detail: Option<&'a str>,
    /// Its description already broken in lines ([`section_detail_lines`], kept by the
    /// caller while its text and width stay), drawn one under the other: when there are
    /// some, drawn instead of `detail`.
    pub lines: &'a [String],
}

/// A section's description broken in up to `lines` lines within `width`, in its font:
/// between words, the last cut with "…" when there's more (for [`SectionTitle::lines`]).
pub fn section_detail_lines(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    detail: &str,
    width: f32,
    lines: usize,
) -> Vec<String> {
    wrap(text, detail, &section_detail_font(ctx), width, lines)
}

/// The font of a section's description.
fn section_detail_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-1.0)
}

/// [`section_title_in`] with its description over several lines (a settings block's, that
/// says more than a line holds: [`SectionTitle::lines`]). Returns the y below it.
pub fn section_title_lines(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    (x, y, width): (f32, f32, f32),
    section: SectionTitle<'_>,
) -> f32 {
    let SectionTitle { title, detail, lines } = section;
    // To a screen reader: a heading named by its title (its detail read under it).
    let heading = Rect::new(x, y, width, ctx.px(SECTION_TITLE_REACH));
    let _told = ui_kit::access::open(|| {
        ui_kit::access::Described::new(ui_kit::access::Role::Heading, title, heading)
    });
    let font = section_title_font(ctx);
    let m = text.metrics(&font);
    let baseline = (y + m.ascent).round();
    draw_ellipsized(layer, text, title, &font, (x, baseline), width, ctx.fade(ctx.palette.text));
    let mut bottom = y + m.ascent + m.descent;
    // The lines given (already fitted), else the one line drawn as it is, cut.
    let one = detail.filter(|_| lines.is_empty());
    let mut shown = one.into_iter().chain(lines.iter().map(String::as_str)).peekable();
    if shown.peek().is_none() {
        return bottom;
    }
    let small = section_detail_font(ctx);
    let sm = text.metrics(&small);
    let color = ctx.fade(ctx.palette.text_muted);
    bottom += ctx.px(3.0);
    for (i, line) in shown.enumerate() {
        if i > 0 {
            bottom += ctx.px(SECTION_DETAIL_LEADING);
        }
        let baseline = (bottom + sm.ascent).round();
        draw_ellipsized(layer, text, line, &small, (x, baseline), width, color);
        bottom += sm.ascent + sm.descent;
    }
    bottom
}

/// The space between two lines of a section's description (logical pixels).
const SECTION_DETAIL_LEADING: f32 = 2.0;

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
    Initials::of(name).as_str().to_owned()
}

/// A name's [`initials`] held on the stack (an avatar drawn every frame writes no text).
#[derive(Debug, Clone, Copy)]
pub struct Initials {
    bytes: [u8; Self::ROOM],
    len: usize,
}

impl Initials {
    /// Room for two letters in upper case (one may become a few: `ß` → `SS`).
    const ROOM: usize = 16;

    pub fn of(name: &str) -> Self {
        let mut words = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
        let mut out = Self { bytes: [0; Self::ROOM], len: 0 };
        match (words.next(), words.next()) {
            (Some(first), Some(second)) => {
                first.chars().take(1).chain(second.chars().take(1)).for_each(|c| out.push_upper(c));
            }
            (Some(only), None) => only.chars().take(2).for_each(|c| out.push_upper(c)),
            _ => out.push_upper('?'),
        }
        out
    }

    fn push_upper(&mut self, c: char) {
        for upper in c.to_uppercase() {
            let mut buf = [0; 4];
            let encoded = upper.encode_utf8(&mut buf).as_bytes();
            let Some(room) = self.bytes.get_mut(self.len..self.len + encoded.len()) else {
                return;
            };
            room.copy_from_slice(encoded);
            self.len += encoded.len();
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or("?")
    }
}
