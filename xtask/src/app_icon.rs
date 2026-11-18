//! `cargo xtask icon`: the app icon at every size, from its raster master
//! `assets/app-icon.png` (1024 × 1024, round art on a transparent background).
//!
//! - `target/icon/<Name>.iconset`: the PNGs `iconutil` turns into the macOS `.icns`
//!   (`scripts/build.sh`). macOS 26 puts any icon that isn't a full squircle into a light
//!   squircle of its own (the round art looked small, on a white tile), so the macOS icon
//!   is Apple's squircle (the 824-of-1024 grid, a soft shadow below) filled with the
//!   painting's black, the round art nearly filling it — as the painting itself is.
//! - `assets/app-icon.ico` (committed; embedded into the Windows executable by
//!   `crates/app/build.rs`) and `assets/app-icon-256.png` (committed; the window icon on
//!   Linux, the Dock's when run outside the bundle). The art nearly fills these: Windows
//!   and Linux don't mask or shrink icons.
//!
//! Sizes up to 48 px don't shrink the whole painting (it would turn to mush): they show a
//! round crop on the cat's head and glowing eye, rimmed in red like the painting. Up to
//! 64 px the result is sharpened a little. The work is done on premultiplied colors, so
//! edges get no dark fringe.

use std::path::{Path, PathBuf};

use image::imageops::{self, FilterType};
use image::{ImageBuffer, Rgba, Rgba32FImage};

/// The sizes in the Windows `.ico` (Explorer, the taskbar, the title bar, at every scale).
pub(crate) const ICO_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
/// The largest size (pixels) showing the crop on the head instead of the whole painting
/// (Windows' 40 and 48, the taskbar and Explorer at 125–150 %, included).
const SMALL_MAX: u32 = 48;
/// The largest size (pixels) sharpened after shrinking.
const SHARPEN_MAX: u32 = 64;
/// The macOS squircle's share of the icon (Apple's grid: 824 of 1024; the rest the shadow).
const MAC_TILE: f32 = 824.0 / 1024.0;
/// The round art's share of the macOS squircle.
const MAC_FILL: f32 = 0.96;
/// The squircle's exponent (|x|ⁿ + |y|ⁿ ≤ 1: Apple's continuous corners, closely).
const SQUIRCLE_N: f32 = 5.0;
/// The squircle's fill: the painting's black.
const TILE_COLOR: [f32; 3] = [0.012, 0.012, 0.018];
/// The art's share of the small sizes (the head crop).
const SMALL_MAC_FILL: f32 = 0.98;
/// The art's share of a Windows / Linux icon (the corners left transparent).
const FLAT_FILL: f32 = 0.98;
/// The crop for the small sizes, in the master's pixels: its centre and radius (the head,
/// both ears and the eye); tighter below 20 px, where the eye must stay a few pixels.
const HEAD_CROP: (f32, f32, f32) = (560.0, 325.0, 240.0);
const TINY_HEAD_CROP: (f32, f32, f32) = (570.0, 330.0, 205.0);
/// The small sizes' rim: the painting's red, and its width (a share of the diameter).
const RIM_COLOR: [f32; 3] = [205.0 / 255.0, 28.0 / 255.0, 36.0 / 255.0];
const RIM_WIDTH: f32 = 0.06;
/// The small sizes are drawn at this multiple of their size, then shrunk (smooth edges).
const SMALL_OVERSAMPLE: u32 = 8;

/// Renders every size; returns `target/icon`.
pub(crate) fn render(root: &Path) -> Result<PathBuf, String> {
    let assets = root.join("assets");
    let master = load(&assets.join("app-icon.png"))?;
    let write = |path: &Path, image: &Rgba32FImage| {
        std::fs::write(path, png(image)?).map_err(|e| format!("{}: {e}", path.display()))
    };

    let out = root.join("target/icon");
    let set = out.join(format!("{}.iconset", kernel::brand::NAME));
    std::fs::create_dir_all(&set).map_err(|e| e.to_string())?;
    for base in [16u32, 32, 128, 256, 512] {
        write(&set.join(format!("icon_{base}x{base}.png")), &mac_icon(&master, base))?;
        let retina = mac_icon(&master, base * 2);
        write(&set.join(format!("icon_{base}x{base}@2x.png")), &retina)?;
    }
    let images = ICO_SIZES
        .iter()
        .map(|&size| Ok((size, png(&icon(&master, size, FLAT_FILL))?)))
        .collect::<Result<Vec<_>, String>>()?;
    let ico_path = assets.join("app-icon.ico");
    std::fs::write(&ico_path, ico(&images)).map_err(|e| format!("{}: {e}", ico_path.display()))?;
    write(&assets.join("app-icon-256.png"), &icon(&master, 256, FLAT_FILL))?;
    Ok(out)
}

/// The macOS icon at `size` × `size`: the squircle (with a soft shadow below) filled with
/// black, the round art inside it.
fn mac_icon(master: &Rgba32FImage, size: u32) -> Rgba32FImage {
    let side = ((size as f32 * MAC_TILE).round() as u32).max(1);
    let tile = squircle(side);
    let mut canvas = Rgba32FImage::new(size, size);
    let at = i64::from((size - side) / 2);
    if size > SMALL_MAX {
        let mut shadow = Rgba32FImage::new(size, size);
        for (x, y, p) in tile.enumerate_pixels() {
            let at = (size - side) / 2;
            shadow.put_pixel(x + at, y + at, Rgba([0.0, 0.0, 0.0, p[3] * 0.3]));
        }
        let shadow = imageops::blur(&shadow, size as f32 * 0.015);
        over(&mut canvas, &shadow, 0, (size as f32 * 0.012).round() as i64);
    }
    over(&mut canvas, &tile, at, at);
    let art = icon(master, side, MAC_FILL);
    over(&mut canvas, &art, at, at);
    canvas
}

/// A squircle `side` × `side` filled with [`TILE_COLOR`] (premultiplied), its edge smooth
/// (4 × 4 samples a pixel).
fn squircle(side: u32) -> Rgba32FImage {
    const SAMPLES: u32 = 4;
    let half = side as f32 / 2.0;
    let mut out = Rgba32FImage::new(side, side);
    for (x, y, p) in out.enumerate_pixels_mut() {
        let mut inside = 0u32;
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let u = ((x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) - half) / half;
                let v = ((y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) - half) / half;
                if u.abs().powf(SQUIRCLE_N) + v.abs().powf(SQUIRCLE_N) <= 1.0 {
                    inside += 1;
                }
            }
        }
        let a = inside as f32 / (SAMPLES * SAMPLES) as f32;
        *p = Rgba([TILE_COLOR[0] * a, TILE_COLOR[1] * a, TILE_COLOR[2] * a, a]);
    }
    out
}

/// The icon at `size` × `size`, the art taking `fill` of it (with a shadow when it leaves
/// room for one: macOS).
fn icon(master: &Rgba32FImage, size: u32, fill: f32) -> Rgba32FImage {
    if size <= SMALL_MAX {
        // Apple's small icons keep a thinner margin (and no room for a shadow).
        return small(master, size, fill.max(SMALL_MAC_FILL));
    }
    let side = ((size as f32 * fill).round() as u32).max(1);
    let mut art = imageops::resize(master, side, side, FilterType::Lanczos3);
    clamp(&mut art);
    if size <= SHARPEN_MAX {
        art = sharpen(&art, 0.6, 0.5);
    }
    let mut canvas = Rgba32FImage::new(size, size);
    let at = i64::from((size - side) / 2);
    if fill < 0.9 {
        // A soft shadow below, as macOS icons have.
        let mut shadow = Rgba32FImage::new(size, size);
        for (x, y, p) in art.enumerate_pixels() {
            shadow.put_pixel(
                x + (size - side) / 2,
                y + (size - side) / 2,
                Rgba([0.0, 0.0, 0.0, p[3] * 0.35]),
            );
        }
        let shadow = imageops::blur(&shadow, size as f32 * 0.012);
        let drop = (size as f32 * 0.012).round() as i64;
        over(&mut canvas, &shadow, 0, drop);
    }
    over(&mut canvas, &art, at, at);
    canvas
}

/// The small sizes: a round crop on the head, livelier colors, a red rim, sharpened.
fn small(master: &Rgba32FImage, size: u32, fill: f32) -> Rgba32FImage {
    let work = size * SMALL_OVERSAMPLE;
    let side = ((work as f32 * fill).round() as u32).max(1);
    let (cx, cy, radius) = if size < 20 { TINY_HEAD_CROP } else { HEAD_CROP };
    let crop = imageops::crop_imm(
        master,
        (cx - radius).max(0.0) as u32,
        (cy - radius).max(0.0) as u32,
        (2.0 * radius) as u32,
        (2.0 * radius) as u32,
    )
    .to_image();
    let mut art = imageops::resize(&crop, side, side, FilterType::Lanczos3);
    clamp(&mut art);
    let center = side as f32 / 2.0;
    let rim = side as f32 * RIM_WIDTH;
    for (x, y, p) in art.enumerate_pixels_mut() {
        let distance =
            ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
        *p = if distance > center {
            Rgba([0.0; 4])
        } else if distance > center - rim {
            Rgba([RIM_COLOR[0], RIM_COLOR[1], RIM_COLOR[2], 1.0])
        } else {
            lively(*p)
        };
    }
    let mut canvas = Rgba32FImage::new(work, work);
    let at = i64::from((work - side) / 2);
    over(&mut canvas, &art, at, at);
    let mut shrunk = imageops::resize(&canvas, size, size, FilterType::Lanczos3);
    clamp(&mut shrunk);
    sharpen(&shrunk, 0.5, 0.6)
}

/// More contrast and saturation (a premultiplied pixel), for the details that survive at
/// a few pixels.
fn lively(p: Rgba<f32>) -> Rgba<f32> {
    let a = p[3];
    if a <= 0.0 {
        return p;
    }
    let rgb = [p[0] / a, p[1] / a, p[2] / a];
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let adjust = |c: f32| {
        let saturated = luma + (c - luma) * 1.2;
        (((saturated - 0.5) * 1.25 + 0.5).clamp(0.0, 1.0)) * a
    };
    Rgba([adjust(rgb[0]), adjust(rgb[1]), adjust(rgb[2]), a])
}

/// Unsharp masking of the colors (the shape, alpha, left as it is).
fn sharpen(image: &Rgba32FImage, sigma: f32, amount: f32) -> Rgba32FImage {
    let blurred = imageops::blur(image, sigma);
    let mut out = image.clone();
    for (p, b) in out.pixels_mut().zip(blurred.pixels()) {
        for c in 0..3 {
            p[c] = (p[c] + amount * (p[c] - b[c])).clamp(0.0, p[3]);
        }
    }
    out
}

/// `top` over `bottom` at (x, y) (premultiplied colors).
fn over(bottom: &mut Rgba32FImage, top: &Rgba32FImage, x: i64, y: i64) {
    let (width, height) = (i64::from(bottom.width()), i64::from(bottom.height()));
    for (tx, ty, p) in top.enumerate_pixels() {
        let (bx, by) = (x + i64::from(tx), y + i64::from(ty));
        if bx < 0 || by < 0 || bx >= width || by >= height {
            continue;
        }
        let q = bottom.get_pixel_mut(bx as u32, by as u32);
        let keep = 1.0 - p[3];
        for c in 0..4 {
            q[c] = p[c] + q[c] * keep;
        }
    }
}

/// Lanczos rings past the valid range: alpha back into 0…1, the colors into 0…alpha.
fn clamp(image: &mut Rgba32FImage) {
    for p in image.pixels_mut() {
        p[3] = p[3].clamp(0.0, 1.0);
        for c in 0..3 {
            p[c] = p[c].clamp(0.0, p[3]);
        }
    }
}

/// The master, premultiplied.
fn load(path: &Path) -> Result<Rgba32FImage, String> {
    let mut image = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba32f();
    for p in image.pixels_mut() {
        for c in 0..3 {
            p[c] *= p[3];
        }
    }
    Ok(image)
}

/// A premultiplied image as an 8-bit RGBA PNG, compressed as much as PNG allows.
fn png(image: &Rgba32FImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
    let pixels: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
            let p = image.get_pixel(x, y);
            let a = p[3];
            let channel = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            let color = |c: f32| if a > 0.0 { channel(c / a) } else { 0 };
            Rgba([color(p[0]), color(p[1]), color(p[2]), channel(a)])
        });
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Best, PngFilter::Adaptive)
        .write_image(&pixels, image.width(), image.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// A Windows icon file holding the given (size, PNG) images (PNG entries: Vista and later).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // 1: an icon (2 would be a cursor).
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, png) in images {
        // 0 stands for 256 (one byte per side).
        let side = u8::try_from(*size).unwrap_or(0);
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // Color planes.
        out.extend_from_slice(&32u16.to_le_bytes()); // Bits per pixel.
        out.extend_from_slice(&u32::try_from(png.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/app_icon.rs"]
mod tests;
