//! Text: shaping, font fallback, rasterization and glyph atlas.
//!
//! The current implementation uses `cosmic-text` (see ADR-0003). The rest of the app
//! only sees [`TextSystem`], [`Font`] and [`ShapedLine`], so the implementation can be
//! replaced without changes to the UI.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
use kernel::{GenerationalCache, Purge};
use render_api::{GlyphFormat, GlyphKey, GlyphRasterizer, RasterizedGlyph};

/// How many shaped lines of a font fit in one cache generation (~10x a
/// full screen).
const LINE_CACHE_LIMIT: usize = 2048;
/// What it may grow to while more lines than that are drawn at once (a big screen of a
/// grid's cells): kept, not shaped again at each frame.
const LINE_CACHE_MOST: usize = 4 * LINE_CACHE_LIMIT;

/// The requested font family.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Monospace,
    SansSerif,
    Named(Arc<str>),
}

/// Concrete font: family + size in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub family: FontFamily,
    pub size: f32,
    /// Weight (100–900; 400 = normal, 600 = semibold, 700 = bold).
    pub weight: u16,
    pub italic: bool,
}

impl Font {
    pub const NORMAL: u16 = 400;
    pub const SEMIBOLD: u16 = 600;
    pub const BOLD: u16 = 700;

    pub fn new(family: FontFamily, size: f32) -> Self {
        Self { family, size, weight: Self::NORMAL, italic: false }
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }
}

/// Metrics of a font at the requested size, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    /// Distance up from the baseline (positive).
    pub ascent: f32,
    /// Distance down from the baseline (positive).
    pub descent: f32,
    /// Width of the `0` character; for monospace fonts, the width of any character.
    pub advance: f32,
}

/// A glyph positioned relative to the start of the line, on the baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub key: GlyphKey,
    pub x: f32,
    pub y: f32,
    /// Byte offset in the shaped text where this glyph's character starts.
    pub start: usize,
    /// Advance width of the glyph.
    pub advance: f32,
}

impl ShapedLine {
    /// X position of the character at offset `byte` (end of line if past the last glyph).
    pub fn x_at(&self, byte: usize) -> f32 {
        self.glyphs.iter().find(|g| g.start >= byte).map_or(self.width, |g| g.x)
    }

    /// Offset of the character closest to `x` (the glyph's midpoint decides the side).
    pub fn byte_at(&self, x: f32, text_len: usize) -> usize {
        self.glyphs.iter().find(|g| x < g.x + g.advance / 2.0).map_or(text_len, |g| g.start)
    }
}

/// Shaping result for one line of text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapedLine {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
}

/// The whole font (family, size, weight, italic): the key of a font's cached lines (no
/// text: the text is the inner key, looked up by `&str`, without allocating) and of its
/// metrics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: FontFamily,
    size_bits: u32,
    weight: u16,
    italic: bool,
}

impl FontKey {
    fn of(font: &Font) -> Self {
        Self {
            family: font.family.clone(),
            size_bits: font.size.to_bits(),
            weight: font.weight,
            italic: font.italic,
        }
    }

    fn matches(&self, font: &Font) -> bool {
        self.size_bits == font.size.to_bits()
            && self.weight == font.weight
            && self.italic == font.italic
            && self.family == font.family
    }
}

/// Shaped lines, grouped by font (fonts are few, so a list); per font, a generational
/// cache (see `kernel::GenerationalCache`), looked up by `&str`. At most
/// [`LINE_CACHE_FONTS`] fonts: zooming (the canvas, the editor) makes a new size at each
/// step, the least recently used one is dropped.
#[derive(Debug, Default)]
struct LineCache {
    fonts: Vec<(FontKey, FontLines)>,
    /// Per font (same order), when it was last used (`tick`).
    used: Vec<u64>,
    tick: u64,
}

/// How many broken faces one line may drop before it's shaped as it is.
const MAX_BROKEN_FACES: usize = 4;

/// How many fonts keep their shaped lines.
const LINE_CACHE_FONTS: usize = 32;

/// Lookups after which a font no one asked for since gives its lines back (the sizes a zoom
/// went through, a dialog's font): about a few minutes of drawing.
const LINE_CACHE_STALE: u64 = 1 << 20;

/// How often (in lookups) the stale fonts are looked for.
const LINE_CACHE_SWEEP: u64 = 1 << 12;

/// A font's lines, keyed by text.
type FontLines = GenerationalCache<Box<str>, Arc<ShapedLine>>;

impl LineCache {
    fn lines(&mut self, font: &Font) -> &mut FontLines {
        self.tick += 1;
        if self.tick.is_multiple_of(LINE_CACHE_SWEEP) {
            self.drop_stale();
        }
        let i = match self.fonts.iter().position(|(k, _)| k.matches(font)) {
            Some(i) => i,
            None => {
                let entry = (
                    FontKey::of(font),
                    GenerationalCache::growing(LINE_CACHE_LIMIT, LINE_CACHE_MOST),
                );
                if self.fonts.len() < LINE_CACHE_FONTS {
                    self.fonts.push(entry);
                    self.used.push(0);
                    self.fonts.len() - 1
                } else {
                    let oldest = (0..self.used.len()).min_by_key(|&i| self.used[i]).unwrap_or(0);
                    self.fonts[oldest] = entry;
                    oldest
                }
            }
        };
        self.used[i] = self.tick;
        &mut self.fonts[i].1
    }

    /// The fonts not used in the last [`LINE_CACHE_STALE`] lookups, with their lines, gone.
    fn drop_stale(&mut self) {
        let since = self.tick.saturating_sub(LINE_CACHE_STALE);
        let mut i = 0;
        while i < self.fonts.len() {
            if self.used[i] < since {
                self.fonts.swap_remove(i);
                self.used.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn len(&self) -> usize {
        self.fonts.iter().map(|(_, l)| l.len()).sum()
    }
}

/// Facade of the text system. Implements [`GlyphRasterizer`] for the renderer.
pub struct TextSystem {
    fonts: FontSystem,
    swash: SwashCache,
    scratch: Buffer,
    glyph_keys: HashMap<CacheKey, GlyphKey>,
    cache_keys: Vec<CacheKey>,
    /// Recently shaped lines. Not cleared all at once (e.g. scrolling a large file
    /// doesn't re-shape the current screen).
    lines: LineCache,
    /// Per whole font: a bold or italic face may be taller or wider than the regular one.
    metrics: HashMap<FontKey, FontMetrics>,
}

impl std::fmt::Debug for TextSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSystem")
            .field("glyphs", &self.cache_keys.len())
            .field("cached_lines", &self.lines.len())
            .finish_non_exhaustive()
    }
}

/// Clears the shaped lines, the metrics and the glyph key table. Glyph keys
/// (`GlyphKey`) are reused after clearing, so the renderer's atlas (see
/// `Renderer::purge_caches`) must be cleared in the same step.
/// Linux: each generic family set to the first of its usual fonts installed, when the
/// default isn't (a minimal system, a CI runner has DejaVu or Liberation, not Open Sans).
/// Liberation Sans before the others: Arial's widths (the texts as wide as on macOS and
/// Windows).
#[cfg(target_os = "linux")]
fn linux_families(db: &mut cosmic_text::fontdb::Database) {
    let has = |db: &cosmic_text::fontdb::Database, name: &str| {
        db.faces().any(|f| f.families.iter().any(|(family, _)| family == name))
    };
    let first = |db: &cosmic_text::fontdb::Database, names: &[&str]| {
        names.iter().find(|n| has(db, n)).map(|n| (*n).to_owned())
    };
    type Set = fn(&mut cosmic_text::fontdb::Database, String);
    let kinds: [(&[&str], Set); 3] = [
        (
            &["Open Sans", "Liberation Sans", "Noto Sans", "DejaVu Sans", "Ubuntu", "Cantarell"],
            |db, n| db.set_sans_serif_family(n),
        ),
        (&["DejaVu Serif", "Noto Serif", "Liberation Serif"], |db, n| db.set_serif_family(n)),
        (&["Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", "Ubuntu Mono"], |db, n| {
            db.set_monospace_family(n)
        }),
    ];
    for (names, set) in kinds {
        if let Some(name) = first(db, names) {
            set(db, name);
        }
    }
}

impl TextSystem {
    /// How many glyph keys were handed out (they're only reset with a purge).
    pub fn glyph_key_count(&self) -> usize {
        self.cache_keys.len()
    }
}

impl Purge for TextSystem {
    fn purge(&mut self) {
        self.lines = LineCache::default();
        self.metrics.clear();
        self.glyph_keys = HashMap::new();
        self.cache_keys = Vec::new();
        self.swash = SwashCache::new();
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSystem {
    /// Loads the system fonts (may take a few tens of ms), for English text fallback.
    pub fn new() -> Self {
        Self::with_locale("en-US")
    }

    /// Loads the system fonts; `locale` (e.g. `ro-RO`) picks the fallback fonts for its
    /// scripts. Given, not asked from the system: that call can block for seconds while
    /// the system's services are busy.
    ///
    /// The system's fonts are looked for once per process; another text system (the tests
    /// make many) gets a copy of that list.
    pub fn with_locale(locale: &str) -> Self {
        static SYSTEM: std::sync::OnceLock<cosmic_text::fontdb::Database> =
            std::sync::OnceLock::new();
        let mut db = SYSTEM
            .get_or_init(|| {
                let mut db = cosmic_text::fontdb::Database::new();
                db.load_system_fonts();
                db
            })
            .clone();
        // The generic families' defaults, as `FontSystem::new` sets them — on Linux, where
        // they often aren't installed, the first font of each kind the system has instead.
        db.set_monospace_family("Noto Sans Mono");
        db.set_sans_serif_family("Open Sans");
        db.set_serif_family("DejaVu Serif");
        #[cfg(target_os = "linux")]
        linux_families(&mut db);
        let mut fonts = FontSystem::new_with_locale_and_db(locale.to_owned(), db);
        let scratch = Buffer::new(&mut fonts, Metrics::new(14.0, 20.0));
        Self {
            fonts,
            swash: SwashCache::new(),
            scratch,
            glyph_keys: HashMap::new(),
            cache_keys: Vec::new(),
            lines: LineCache::default(),
            metrics: HashMap::new(),
        }
    }

    /// A text system with only `fonts` (no system font: the same glyphs on every system),
    /// e.g. a formula's math font, loaded and dropped with what uses it.
    pub fn with_fonts(fonts: Vec<Vec<u8>>) -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        for font in fonts {
            db.load_font_data(font);
        }
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".to_owned(), db);
        let scratch = Buffer::new(&mut fonts, Metrics::new(14.0, 20.0));
        Self {
            fonts,
            swash: SwashCache::new(),
            scratch,
            glyph_keys: HashMap::new(),
            cache_keys: Vec::new(),
            lines: LineCache::default(),
            metrics: HashMap::new(),
        }
    }

    /// The families of the fonts it has (their first names).
    pub fn families(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for face in self.fonts.db().faces() {
            if let Some((name, _)) = face.families.first()
                && !out.contains(name)
            {
                out.push(name.clone());
            }
        }
        out
    }

    /// The first family in the list that is installed on the system, otherwise `fallback`.
    pub fn first_available(&self, candidates: &[&str], fallback: FontFamily) -> FontFamily {
        let db = self.fonts.db();
        candidates
            .iter()
            .find(|name| db.faces().any(|face| face.families.iter().any(|(f, _)| f == *name)))
            .map(|name| FontFamily::Named(Arc::from(*name)))
            .unwrap_or(fallback)
    }

    /// Finds and loads the faces for `fonts` ahead (a background thread, before the first
    /// frame needs them): picking a face reads through every installed font, ~10 ms per
    /// family, weight and style the first time. Only the font system's own caches fill up
    /// (the shaped lines, the metrics and the glyph keys stay as they are).
    pub fn warm_up(&mut self, fonts: &[Font]) {
        for font in fonts {
            let style =
                if font.italic { cosmic_text::Style::Italic } else { cosmic_text::Style::Normal };
            let attrs = Attrs::new()
                .family(family(&font.family))
                .weight(cosmic_text::Weight(font.weight))
                .style(style);
            self.scratch.set_metrics(Metrics::new(font.size, font.size));
            self.scratch.set_size(None, None);
            self.scratch.set_text("Aa0", &attrs, Shaping::Advanced, None);
            self.scratch.shape_until_scroll(&mut self.fonts, false);
        }
    }

    /// Shapes one line (without `\n`). The result is kept in the cache.
    pub fn shape_line(&mut self, text: &str, font: &Font) -> Arc<ShapedLine> {
        if let Some(line) = self.lines.lines(font).get(text) {
            return Arc::clone(line);
        }
        let line = Arc::new(self.shape_uncached(text, font));
        self.lines.lines(font).insert(text.into(), Arc::clone(&line));
        line
    }

    pub fn metrics(&mut self, font: &Font) -> FontMetrics {
        let key = FontKey::of(font);
        if let Some(m) = self.metrics.get(&key) {
            return *m;
        }
        let m = self.measure(font);
        self.metrics.insert(key, m);
        m
    }

    fn shape_uncached(&mut self, text: &str, font: &Font) -> ShapedLine {
        let style =
            if font.italic { cosmic_text::Style::Italic } else { cosmic_text::Style::Normal };
        let attrs = Attrs::new()
            .family(family(&font.family))
            .weight(cosmic_text::Weight(font.weight))
            .style(style);
        self.scratch.set_metrics(Metrics::new(font.size, font.size));
        self.scratch.set_size(None, None);
        self.scratch.set_text(text, &attrs, Shaping::Advanced, None);
        self.scratch.shape_until_scroll(&mut self.fonts, false);
        // A face the system calls monospaced whose space has no width (macOS has a bitmap
        // one) gets its fallback glyphs sized infinite by the monospace matching: it leaves
        // the fonts, once, and the line is shaped again without it.
        for _ in 0..MAX_BROKEN_FACES {
            let broken: Vec<cosmic_text::fontdb::ID> = self
                .scratch
                .layout_runs()
                .flat_map(|run| run.glyphs.iter())
                // Its own size, not its place: the glyphs after it are pushed away too.
                .filter(|g| !(g.w.is_finite() && g.font_size.is_finite()))
                .map(|g| g.font_id)
                .collect();
            if broken.is_empty() {
                break;
            }
            self.drop_faces(&broken);
            self.scratch.set_text(text, &attrs, Shaping::Advanced, None);
            self.scratch.shape_until_scroll(&mut self.fonts, false);
        }

        let mut line = ShapedLine::default();
        // As many glyphs as the runs hold, at once (kept in the line cache: no room left over
        // from doubling as they're pushed).
        line.glyphs.reserve_exact(self.scratch.layout_runs().map(|run| run.glyphs.len()).sum());
        for run in self.scratch.layout_runs() {
            line.width = line.width.max(run.line_w);
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let key = intern(&mut self.glyph_keys, &mut self.cache_keys, physical.cache_key);
                line.glyphs.push(ShapedGlyph {
                    key,
                    x: physical.x as f32,
                    y: physical.y as f32,
                    start: glyph.start,
                    advance: glyph.w,
                });
            }
        }
        line
    }

    /// The fonts without `faces` (the others keep their ids: what's cached of them stays).
    fn drop_faces(&mut self, faces: &[cosmic_text::fontdb::ID]) {
        let locale = self.fonts.locale().to_owned();
        let empty = FontSystem::new_with_locale_and_db(
            locale.clone(),
            cosmic_text::fontdb::Database::new(),
        );
        let (_, mut db) = std::mem::replace(&mut self.fonts, empty).into_locale_and_db();
        for &id in faces {
            tracing::warn!(
                "font {:?} left out: its glyphs can't be sized",
                db.face(id).map(|f| f.post_script_name.clone())
            );
            db.remove_face(id);
        }
        self.fonts = FontSystem::new_with_locale_and_db(locale, db);
    }

    fn measure(&mut self, font: &Font) -> FontMetrics {
        let style =
            if font.italic { cosmic_text::Style::Italic } else { cosmic_text::Style::Normal };
        let attrs = Attrs::new()
            .family(family(&font.family))
            .weight(cosmic_text::Weight(font.weight))
            .style(style);
        self.scratch.set_metrics(Metrics::new(font.size, font.size));
        self.scratch.set_size(None, None);
        self.scratch.set_text("0", &attrs, Shaping::Advanced, None);
        self.scratch.shape_until_scroll(&mut self.fonts, false);

        let glyph = self.scratch.layout_runs().flat_map(|r| r.glyphs.iter()).next().cloned();
        let fallback = FontMetrics {
            ascent: font.size * 0.8,
            descent: font.size * 0.2,
            advance: font.size * 0.6,
        };
        let Some(glyph) = glyph else { return fallback };
        let Some(face) = self.fonts.get_font(glyph.font_id, glyph.font_weight) else {
            return fallback;
        };
        let m = face.metrics();
        let scale = font.size / f32::from(m.units_per_em.max(1));
        FontMetrics { ascent: m.ascent * scale, descent: -m.descent * scale, advance: glyph.w }
    }
}

impl GlyphRasterizer for TextSystem {
    fn rasterize(&mut self, key: GlyphKey) -> Option<RasterizedGlyph> {
        let cache_key = *self.cache_keys.get(usize::try_from(key.0).ok()?)?;
        let image = self.swash.get_image_uncached(&mut self.fonts, cache_key)?;
        let p = image.placement;
        let (format, data) = match image.content {
            SwashContent::Mask => (GlyphFormat::Mask, image.data),
            SwashContent::Color => (GlyphFormat::Color, image.data),
            // We don't request subpixel rendering; if it shows up anyway, reduce it to a mask.
            SwashContent::SubpixelMask => {
                let mask = image.data.as_chunks::<4>().0.iter().map(|px| px[1]).collect();
                (GlyphFormat::Mask, mask)
            }
        };
        Some(RasterizedGlyph {
            left: p.left,
            top: p.top,
            width: p.width,
            height: p.height,
            format,
            data,
        })
    }
}

fn family(f: &FontFamily) -> Family<'_> {
    match f {
        FontFamily::Monospace => Family::Monospace,
        FontFamily::SansSerif => Family::SansSerif,
        FontFamily::Named(name) => Family::Name(name),
    }
}

fn intern(
    keys: &mut HashMap<CacheKey, GlyphKey>,
    cache_keys: &mut Vec<CacheKey>,
    cache_key: CacheKey,
) -> GlyphKey {
    *keys.entry(cache_key).or_insert_with(|| {
        cache_keys.push(cache_key);
        GlyphKey(cache_keys.len() as u64 - 1)
    })
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
