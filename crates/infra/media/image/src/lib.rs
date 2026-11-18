//! Decoding images into RGBA8 pixels (non-premultiplied), for viewers.
//! Very large images are downscaled while decoding (memory and the GPU texture limit).
//! A photo's EXIF orientation is applied (its sizes are the ones shown). An SVG's texts
//! are set with the system's fonts, listed once, on the worker decoding the first SVG that
//! has a text (~90 ms, ~2 MB), then shared by every decode.

use std::io;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Recognized extensions (lowercase).
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"];

/// Maximum side of the decoded image.
pub const MAX_SIDE: u32 = 4096;

/// The largest image file read (a bigger one isn't an image to show).
pub const MAX_FILE: u64 = 256 * 1024 * 1024;

/// The largest image decoded, per side, before downscaling (a tiny file can claim any size:
/// its full pixels must fit in memory while decoding).
const MAX_DECODED_SIDE: u32 = 16_384;

/// A decoded image.
#[derive(Clone)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, non-premultiplied RGBA.
    pub pixels: Vec<u8>,
    /// The original size (before downscaling), for display.
    pub original: (u32, u32),
    /// The format, e.g. `PNG`.
    pub format: &'static str,
}

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decoded {}x{} {}", self.width, self.height, self.format)
    }
}

/// The file looks like an image (by extension).
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The file is an SVG (by extension).
pub fn is_svg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// Reads and decodes the image at `path`.
pub fn load(path: &Path) -> io::Result<Decoded> {
    load_fitting(path, MAX_SIDE)
}

/// Reads and decodes the image at `path`, downscaled so that no side exceeds `max_side`
/// (the full pixels exist only while decoding).
pub fn load_fitting(path: &Path, max_side: u32) -> io::Result<Decoded> {
    let bytes = read(path)?;
    let max_side = max_side.clamp(1, MAX_SIDE);
    if is_svg(path) {
        decode_svg_fitting(&bytes, max_side)
    } else {
        decode_fitting(&bytes, max_side)
    }
}

/// Reads and decodes the image at `path` at most `width` pixels wide (its proportions
/// kept; an SVG drawn at its own size × `svg_scale`, at most that wide): an image shown
/// smaller than it is never holds more than it shows.
pub fn load_for_width(path: &Path, width: u32, svg_scale: f32) -> io::Result<Decoded> {
    load_for_width_from(path, width, svg_scale, false)
}

/// [`load_for_width`] for a file from elsewhere (the web): an SVG loads nothing it names
/// (no files, no other images but those inside it).
pub fn load_untrusted_for_width(path: &Path, width: u32, svg_scale: f32) -> io::Result<Decoded> {
    load_for_width_from(path, width, svg_scale, true)
}

/// The system's fonts, for an SVG's texts: listed once, by the first decode that needs
/// them (on its worker), shared by all after.
fn system_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        generic_families(&mut db);
        Arc::new(db)
    }))
}

/// The generic families (`sans-serif`, `serif`, `monospace`) set to fonts the system has
/// where its defaults (Arial, Times New Roman, Courier New) aren't there — most Linux
/// systems: an SVG's text would otherwise be drawn with nothing.
fn generic_families(db: &mut resvg::usvg::fontdb::Database) {
    let has = |db: &resvg::usvg::fontdb::Database, name: &str| {
        db.faces().any(|f| f.families.iter().any(|(family, _)| family == name))
    };
    let first = |db: &resvg::usvg::fontdb::Database, names: &[&str]| {
        names.iter().find(|n| has(db, n)).map(|n| (*n).to_owned())
    };
    if !has(db, "Arial")
        && let Some(name) = first(
            db,
            &["Liberation Sans", "Noto Sans", "DejaVu Sans", "Ubuntu", "Cantarell", "Open Sans"],
        )
    {
        db.set_sans_serif_family(name);
    }
    if !has(db, "Times New Roman")
        && let Some(name) = first(db, &["Noto Serif", "DejaVu Serif", "Liberation Serif"])
    {
        db.set_serif_family(name);
    }
    if !has(db, "Courier New")
        && let Some(name) =
            first(db, &["Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", "Ubuntu Mono"])
    {
        db.set_monospace_family(name);
    }
}

/// The SVG has a text to set (`<text`, or a namespaced `<svg:text`): only then are the
/// system's fonts listed.
fn has_text(bytes: &[u8]) -> bool {
    bytes.windows(5).any(|w| w == b"<text" || w == b":text")
}

/// How an SVG is read: with the system's fonts when it has texts; from elsewhere
/// (`untrusted`), loading nothing it names (no files, no other images but those inside it).
fn svg_options(bytes: &[u8], untrusted: bool) -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    if has_text(bytes) {
        options.fontdb = system_fonts();
    }
    if untrusted {
        options.image_href_resolver = resvg::usvg::ImageHrefResolver {
            resolve_data: resvg::usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        };
    }
    options
}

/// Reads and draws the SVG at `path` with its long side `side` pixels (at most
/// [`MAX_SIDE`]), larger or smaller than its own size: as sharp as it's shown.
pub fn load_svg_at_side(path: &Path, side: u32) -> io::Result<Decoded> {
    let bytes = read(path)?;
    let tree = resvg::usvg::Tree::from_data(&bytes, &svg_options(&bytes, false))
        .map_err(io::Error::other)?;
    let long = tree.size().width().max(tree.size().height()).max(1.0);
    let scale = side.clamp(1, MAX_SIDE) as f32 / long;
    decode_svg_scaled(&tree, scale)
}

fn load_for_width_from(
    path: &Path,
    width: u32,
    svg_scale: f32,
    untrusted: bool,
) -> io::Result<Decoded> {
    let bytes = read(path)?;
    let width = width.clamp(1, MAX_SIDE);
    if is_svg(path) {
        let options = svg_options(&bytes, untrusted);
        let tree = resvg::usvg::Tree::from_data(&bytes, &options).map_err(io::Error::other)?;
        let w = tree.size().width().max(1.0);
        let h = tree.size().height().max(1.0);
        let scale = (width as f32 / w).min(svg_scale).min(MAX_SIDE as f32 / w.max(h));
        return decode_svg_scaled(&tree, scale);
    }
    let reader = image::ImageReader::new(io::Cursor::new(&bytes)).with_guessed_format()?;
    let (w, h) = oriented_size(reader)?;
    let side = if w > width {
        (u64::from(w.max(h)) * u64::from(width)).div_ceil(u64::from(w)) as u32
    } else {
        w.max(h)
    };
    decode_fitting(&bytes, side.clamp(1, MAX_SIDE))
}

/// An SVG drawn at `scale`.
fn decode_svg_scaled(tree: &resvg::usvg::Tree, scale: f32) -> io::Result<Decoded> {
    let size = tree.size();
    let original = (size.width().ceil() as u32, size.height().ceil() as u32);
    // Never a side over the limit (a rounding up, a long thin drawing).
    let w = ((size.width() * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let h = ((size.height() * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| io::Error::other("SVG too large"))?;
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Decoded { width: pixmap.width(), height: pixmap.height(), pixels, original, format: "SVG" })
}

/// The bytes of the image file at `path`: only a regular file (a device such as `/dev/zero`
/// or a pipe never ends), at most [`MAX_FILE`].
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    use io::Read;
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
    }
    if meta.len() > MAX_FILE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large for an image"));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(path)?.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large for an image"));
    }
    Ok(bytes)
}

/// The original size and the format, from the file's header only (nothing is decoded).
pub fn probe(path: &Path) -> io::Result<((u32, u32), &'static str)> {
    if is_svg(path) {
        let bytes = read(path)?;
        let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default())
            .map_err(io::Error::other)?;
        let size = tree.size();
        return Ok(((size.width().ceil() as u32, size.height().ceil() as u32), "SVG"));
    }
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let format = format_name(reader.format());
    Ok((oriented_size(reader)?, format))
}

/// The image's size as shown: its EXIF orientation applied (sides swapped when it turns a
/// quarter).
fn oriented_size<R: io::BufRead + io::Seek>(
    reader: image::ImageReader<R>,
) -> io::Result<(u32, u32)> {
    use image::ImageDecoder;
    let mut decoder = reader.into_decoder().map_err(io::Error::other)?;
    let (w, h) = decoder.dimensions();
    let turned = matches!(
        decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms),
        image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH
    );
    Ok(if turned { (h, w) } else { (w, h) })
}

/// The image decoded whole, its EXIF orientation applied (an unreadable tag: as stored).
fn decode_oriented<R: io::BufRead + io::Seek>(
    reader: image::ImageReader<R>,
) -> io::Result<image::DynamicImage> {
    use image::ImageDecoder;
    let mut decoder = reader.into_decoder().map_err(io::Error::other)?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(io::Error::other)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn format_name(format: Option<image::ImageFormat>) -> &'static str {
    match format {
        Some(image::ImageFormat::Png) => "PNG",
        Some(image::ImageFormat::Jpeg) => "JPEG",
        Some(image::ImageFormat::Gif) => "GIF",
        Some(image::ImageFormat::WebP) => "WebP",
        Some(image::ImageFormat::Bmp) => "BMP",
        Some(image::ImageFormat::Ico) => "ICO",
        _ => "imagine",
    }
}

/// Decodes a raster image (the format is guessed from the content).
pub fn decode(bytes: &[u8]) -> io::Result<Decoded> {
    decode_fitting(bytes, MAX_SIDE)
}

/// Decodes a raster image, downscaled so that no side exceeds `max_side`.
pub fn decode_fitting(bytes: &[u8], max_side: u32) -> io::Result<Decoded> {
    let mut reader = image::ImageReader::new(io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(io::Error::other)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    reader.limits(limits);
    let format = format_name(reader.format());
    let image = decode_oriented(reader)?;
    let original = (image.width(), image.height());
    // `thumbnail`: area averaging, fast and sharp for large reductions.
    let image = if original.0 > max_side || original.1 > max_side {
        image.thumbnail(max_side, max_side)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
        original,
        format,
    })
}

/// Renders an SVG at its own size (scaled up to 1024 px on the long side, so it
/// stays sharp when shown large).
pub fn decode_svg(bytes: &[u8]) -> io::Result<Decoded> {
    decode_svg_fitting(bytes, MAX_SIDE)
}

/// Renders an SVG (as [`decode_svg`]) with no side over `max_side`.
pub fn decode_svg_fitting(bytes: &[u8], max_side: u32) -> io::Result<Decoded> {
    let tree = resvg::usvg::Tree::from_data(bytes, &svg_options(bytes, false))
        .map_err(io::Error::other)?;
    let size = tree.size();
    let original = (size.width().ceil() as u32, size.height().ceil() as u32);
    let side = size.width().max(size.height()).max(1.0);
    let scale = (1024.0 / side).clamp(1.0, 8.0).min(max_side as f32 / side);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))
        .ok_or_else(|| io::Error::other("SVG too large"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps pixels premultiplied.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Decoded { width: pixmap.width(), height: pixmap.height(), pixels, original, format: "SVG" })
}

/// An image transform: 90° rotations and flips, composed into a canonical form
/// (`rotations` × 90° clockwise, then a horizontal flip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transform {
    pub rotations: u8,
    pub flipped: bool,
}

impl Transform {
    pub fn is_identity(self) -> bool {
        self.rotations.is_multiple_of(4) && !self.flipped
    }

    /// 90° rotation (`clockwise` or counterclockwise), applied after the current transform.
    pub fn rotate(self, clockwise: bool) -> Self {
        // With a flip, a rotation after it is the inverse rotation before it.
        let step: u8 = if clockwise ^ self.flipped { 1 } else { 3 };
        Self { rotations: (self.rotations + step) % 4, flipped: self.flipped }
    }

    pub fn flip_horizontal(self) -> Self {
        Self { flipped: !self.flipped, ..self }
    }

    /// Vertical flip = horizontal flip + 180° rotation.
    pub fn flip_vertical(self) -> Self {
        Self { rotations: (self.rotations + 2) % 4, flipped: !self.flipped }
    }

    /// The size after the transform.
    pub fn size(self, width: u32, height: u32) -> (u32, u32) {
        if !self.rotations.is_multiple_of(2) { (height, width) } else { (width, height) }
    }

    fn apply_dynamic(self, image: image::DynamicImage) -> image::DynamicImage {
        let image = match self.rotations % 4 {
            1 => image.rotate90(),
            2 => image.rotate180(),
            3 => image.rotate270(),
            _ => image,
        };
        if self.flipped { image.fliph() } else { image }
    }

    /// The transformed RGBA pixels.
    pub fn apply(self, width: u32, height: u32, pixels: &[u8]) -> (u32, u32, Vec<u8>) {
        self.apply_owned(width, height, pixels.to_vec())
    }

    /// The transformed RGBA pixels, consuming them (no copy when there's no transform).
    pub fn apply_owned(self, width: u32, height: u32, pixels: Vec<u8>) -> (u32, u32, Vec<u8>) {
        if self.is_identity() {
            return (width, height, pixels);
        }
        let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels) else {
            return (width, height, Vec::new());
        };
        let out = self.apply_dynamic(image::DynamicImage::ImageRgba8(buffer)).into_rgba8();
        (out.width(), out.height(), out.into_raw())
    }
}

/// A smaller copy of RGBA pixels, with no side over `max_side` (as is if already small).
pub fn shrink(width: u32, height: u32, pixels: &[u8], max_side: u32) -> (u32, u32, Vec<u8>) {
    if width <= max_side && height <= max_side {
        return (width, height, pixels.to_vec());
    }
    let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels.to_vec()) else {
        return (0, 0, Vec::new());
    };
    let (w, h) = if width >= height {
        (max_side, (height * max_side / width).max(1))
    } else {
        ((width * max_side / height).max(1), max_side)
    };
    let out = image::imageops::thumbnail(&buffer, w, h);
    (out.width(), out.height(), out.into_raw())
}

/// The file can be rewritten after the transform (raster formats with an encoder).
pub fn can_save(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "ico")
}

/// Why the file can't be written back from one picture without losing what it holds:
/// an animation (GIF, APNG, WebP: only its first frame would stay) or an icon of several
/// sizes (only one would stay). `None`: it can.
fn holds_more_than_one(bytes: &[u8]) -> Option<&'static str> {
    use image::AnimationDecoder;
    let cursor = || io::Cursor::new(bytes);
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Gif => {
            let frames = image::codecs::gif::GifDecoder::new(cursor()).ok()?.into_frames();
            (frames.take(2).count() > 1).then_some("an animated image can't be saved")
        }
        image::ImageFormat::Png => image::codecs::png::PngDecoder::new(cursor())
            .ok()?
            .is_apng()
            .ok()?
            .then_some("an animated image can't be saved"),
        image::ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(cursor())
            .ok()?
            .has_animation()
            .then_some("an animated image can't be saved"),
        // The icon directory's count: an `ICONDIR` of more than one entry.
        image::ImageFormat::Ico => {
            let count = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]);
            (count > 1).then_some("an icon of several sizes can't be saved")
        }
        _ => None,
    }
}

/// Applies the transform to the file, at the original resolution, in the same format.
/// Writing goes through a temporary file, so an error doesn't leave a broken image. The
/// pixels are written as shown (a photo's EXIF orientation applied, and no orientation
/// written: the encoders write no EXIF). A file holding more than one picture (an
/// animation, an icon of several sizes) is refused rather than written as one.
pub fn save_transformed(path: &Path, transform: Transform) -> io::Result<()> {
    if !can_save(path) {
        return Err(io::Error::other("this format can't be saved"));
    }
    let bytes = read(path)?;
    if let Some(why) = holds_more_than_one(&bytes) {
        return Err(io::Error::new(io::ErrorKind::Unsupported, why));
    }
    let reader = image::ImageReader::new(io::Cursor::new(&bytes)).with_guessed_format()?;
    let image = decode_oriented(reader)?;
    drop(bytes);
    let format = image::ImageFormat::from_path(path).map_err(io::Error::other)?;
    let out = transform.apply_dynamic(image);
    let tmp = path.with_extension("saving-tmp");
    let result = match format {
        image::ImageFormat::Jpeg => {
            let file = std::fs::File::create(&tmp)?;
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(io::BufWriter::new(file), 92);
            encoder.encode_image(&out.to_rgb8()).map_err(io::Error::other)
        }
        format => out.save_with_format(&tmp, format).map_err(io::Error::other),
    };
    match result {
        Ok(()) => std::fs::rename(&tmp, path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
