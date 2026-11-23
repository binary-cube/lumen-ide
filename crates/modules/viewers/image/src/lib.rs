//! Image module: PNG, JPEG, GIF, WebP, BMP, ICO and SVG files open as
//! images (not as text). The image is a GPU texture: fitted to the panel (or at its
//! real size, if smaller), with zoom, panning, rotation and flipping.
//!
//! Toolbar (top): zoom − / percent / zoom +, fit, 1:1, rotate left /
//! right, flip horizontal / vertical, background (checkerboard / dark / light),
//! save. Keys: `+` `-` `0` (fit) `1` (1:1) `r` / `R` (rotate) `h` `v`
//! (flip) `b` (background); ⌘S saves the transformed image at the original resolution.

mod toolbar;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use image_decode::Transform;
use kernel::{Point, Rect, Rgba};
use languages::IconSvg;
use render_api::{ImageData, Quad};
use text_layout::Font;
use ui_components::{Painter, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerProvider};
use ui_kit::{centered_baseline, draw_text};
use workbench_model::{CommandContribution, ViewAction};

use crate::toolbar::{Tool, Toolbar};

/// The images being decoded: two at a time (many opened at once don't start a thread each).
static DECODES: std::sync::LazyLock<kernel::Pool> =
    std::sync::LazyLock::new(|| kernel::Pool::new("image-decode", 2, kernel::work::IDLE));

/// The image files read (the Task Manager's disk column).
static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("viewer.image"),
    name: "Images",
    description: "Opens images (PNG, JPEG, GIF, WebP, BMP, ICO, SVG) as images, with zoom, rotation, flipping and saving",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "image", ..extensions::ModuleInfo::BUILT_IN },
};

/// The dark ground under an image (its transparency seen against it).
const DARK_GROUND: Rgba = Rgba::rgb(0x1e1f22);

const ICONS: &[IconSvg] = &[
    IconSvg { name: "img-zoom-in", svg: include_str!("../svg/img-zoom-in.svg") },
    IconSvg { name: "img-zoom-out", svg: include_str!("../svg/img-zoom-out.svg") },
    IconSvg { name: "img-fit", svg: include_str!("../svg/img-fit.svg") },
    IconSvg { name: "img-actual", svg: include_str!("../svg/img-actual.svg") },
    IconSvg { name: "img-rotate-ccw", svg: include_str!("../svg/img-rotate-ccw.svg") },
    IconSvg { name: "img-rotate-cw", svg: include_str!("../svg/img-rotate-cw.svg") },
    IconSvg { name: "img-flip-h", svg: include_str!("../svg/img-flip-h.svg") },
    IconSvg { name: "img-flip-v", svg: include_str!("../svg/img-flip-v.svg") },
    IconSvg { name: "img-background", svg: include_str!("../svg/img-background.svg") },
    IconSvg { name: "img-save", svg: include_str!("../svg/img-save.svg") },
];

#[derive(Debug, Default)]
pub struct ImageViewerModule;

impl Module for ImageViewerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["image-decode"]));
        ctx.contribute(workbench_model::ModuleCounters { disk: Some(&DISK), traffic: None });
        for icon in ICONS {
            ctx.contribute(*icon);
        }
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(ImageProvider));
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The image's viewer (where its actions' keys work).
const VIEW: &str = "image";

/// The image's actions (remappable; the keys they always had): the command, its label, its
/// keys, what the view is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 9] = [
    (CommandId("image.zoom_in"), "Zoom In (Image)", &["=", "Shift+="], "zoom_in"),
    (CommandId("image.zoom_out"), "Zoom Out (Image)", &["-", "Shift+-"], "zoom_out"),
    (CommandId("image.fit"), "Fit (Image)", &["0"], "fit"),
    (CommandId("image.actual"), "Actual Size (Image)", &["1"], "actual"),
    (CommandId("image.rotate_right"), "Rotate Right", &["r"], "rotate_right"),
    (CommandId("image.rotate_left"), "Rotate Left", &["Shift+R"], "rotate_left"),
    (CommandId("image.flip_horizontal"), "Flip Horizontally", &["h"], "flip_horizontal"),
    (CommandId("image.flip_vertical"), "Flip Vertically", &["v"], "flip_vertical"),
    (CommandId("image.background"), "Background (Image)", &["b"], "background"),
];

#[derive(Debug)]
struct ImageProvider;

impl ViewerProvider for ImageProvider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        image_decode::is_image(path).then_some(ViewerKind::Replace)
    }

    /// Only the header is read here: the pixels are decoded when the image is shown.
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ImageViewer::open(path)?))
    }
}

impl ImageViewer {
    /// The viewer of `path`, from its header (its size and format).
    fn open(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let (original, format) = image_decode::probe(path)?;
        Ok(ImageViewer {
            path: path.to_path_buf(),
            svg: image_decode::is_svg(path),
            generation: Arc::new(AtomicU64::new(0)),
            shown: None,
            thumb: None,
            thumb_shown: None,
            decoding: None,
            saving: None,
            waker: None,
            failed: false,
            original,
            format,
            bytes,
            transform: Transform::default(),
            zoom: None,
            pan: Point::new(0.0, 0.0),
            bounds: Rect::default(),
            scale: 1.0,
            background: Background::Checker,
            toolbar: Toolbar::default(),
            message: None,
        })
    }
}

/// Its view closed (its tab, the window, the app quitting) while it's written: the save
/// finishes first (never a quit cutting it short), at most [`SAVE_WAIT`]. Taken by a poll,
/// it has nothing more to wait for.
impl Drop for Saving {
    fn drop(&mut self) {
        let _ = self.result.recv_timeout(SAVE_WAIT);
    }
}

/// The longest a closing view waits for its save.
const SAVE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// What is visible beneath the image (for transparent areas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Background {
    Checker,
    Dark,
    Light,
}

/// The image as drawn: decoded at `side` (long side, ≤ the original), with `transform`.
/// Its pixels live only on the GPU (`ImageData::gpu_only`).
struct Shown {
    image: ImageData,
    side: u32,
    transform: Transform,
}

/// A save running in the background: the transform it writes, and its result (the file's
/// new size).
struct Saving {
    transform: Transform,
    result: Receiver<std::io::Result<u64>>,
}

/// A decode running in the background: the long side asked for, and the result.
struct Decoding {
    side: u32,
    result: Receiver<std::io::Result<image_decode::Decoded>>,
}

/// The long side of the small copy kept in memory (drawn while the image is decoded).
const THUMB_SIDE: u32 = 160;
/// Below this, an image is never decoded smaller (it's cheap anyway).
const MIN_SIDE: u32 = 512;

/// An SVG is drawn in steps of this many pixels (its long side).
const SVG_STEP: u32 = 64;

/// What a decode needs and asks for: the long side under which the image is blurry, and
/// the one decoded (with room, so zooming in by little doesn't decode again).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    need: u32,
    ask: u32,
}

struct ImageViewer {
    path: PathBuf,
    /// An SVG: drawn again, as sharp as it's shown, at any zoom.
    svg: bool,
    /// The latest decode asked for: an older one still queued is let go.
    generation: Arc<AtomicU64>,
    shown: Option<Shown>,
    /// A small untransformed copy, kept in memory.
    thumb: Option<ImageData>,
    /// The small copy with the current transform.
    thumb_shown: Option<(Transform, ImageData)>,
    decoding: Option<Decoding>,
    /// The transform being written to the file (read, turned, encoded and written on a
    /// worker: never the UI's wait). Meanwhile the image is shown as it is, the turning
    /// tools wait, and nothing is unsaved.
    saving: Option<Saving>,
    /// Wakes the app when a save is done (the paint's).
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The file couldn't be decoded (not retried on every frame).
    failed: bool,
    original: (u32, u32),
    format: &'static str,
    bytes: u64,
    transform: Transform,
    /// Physical pixels per image pixel; `None` = auto-fit to the panel.
    zoom: Option<f32>,
    /// Offset from the center (physical pixels), when the image is larger than the panel.
    pan: Point,
    bounds: Rect,
    scale: f32,
    background: Background,
    toolbar: Toolbar,
    /// A short message in the bottom badge (e.g. after saving).
    message: Option<String>,
}

impl ImageViewer {
    /// Original size, after rotation.
    fn size(&self) -> (f32, f32) {
        let (w, h) = self.transform.size(self.original.0.max(1), self.original.1.max(1));
        (w as f32, h as f32)
    }

    /// Automatic zoom: fits the panel, but no larger than the real size (in logical
    /// pixels, so that on Retina screens it does not look half as large).
    fn fit(&self) -> f32 {
        // Room for the toolbar (top) and the info badge (bottom).
        let (mx, my) = (96.0 * self.scale, 150.0 * self.scale);
        let (w, h) = self.size();
        let fit =
            ((self.bounds.size.width - mx) / w).min((self.bounds.size.height - my) / h).max(0.01);
        fit.min(self.scale)
    }

    fn current_zoom(&self) -> f32 {
        self.zoom.unwrap_or_else(|| self.fit())
    }

    /// Where the image is drawn.
    fn rect(&self) -> Rect {
        let z = self.current_zoom();
        let (w, h) = self.size();
        let (w, h) = (w * z, h * z);
        let b = self.bounds;
        let x = b.min_x() + (b.size.width - w) / 2.0 + self.pan.x;
        let y = b.min_y() + (b.size.height - h) / 2.0 + self.pan.y;
        Rect::new(x.round(), y.round(), w.round(), h.round())
    }

    /// The offset stays within bounds: the image cannot escape the panel.
    fn clamp_pan(&mut self) {
        let z = self.current_zoom();
        let (w, h) = self.size();
        let max_x = ((w * z - self.bounds.size.width) / 2.0 + 24.0 * self.scale).max(0.0);
        let max_y = ((h * z - self.bounds.size.height) / 2.0 + 24.0 * self.scale).max(0.0);
        self.pan = Point::new(self.pan.x.clamp(-max_x, max_x), self.pan.y.clamp(-max_y, max_y));
    }

    fn center(&self) -> Point {
        Point::new(
            self.bounds.min_x() + self.bounds.size.width / 2.0,
            self.bounds.min_y() + self.bounds.size.height / 2.0,
        )
    }

    /// Changes the transform and recomputes the displayed pixels (only once, not every
    /// frame; the old texture is freed from the GPU on its own).
    fn set_transform(&mut self, transform: Transform) {
        // The pixels are on the GPU only: the transformed image is decoded again (in the
        // background, at the shown size); the small copy is drawn meanwhile.
        self.transform = transform;
        self.pan = Point::new(0.0, 0.0);
        self.message = None;
    }

    /// The long side to decode at so that the image is sharp at the current zoom. A raster
    /// image: the original halved while that still covers the shown size (so zooming in
    /// decodes again only past each step). An SVG: drawn at the shown size in physical
    /// pixels (a Retina screen's too), a quarter more in steps of [`SVG_STEP`], and drawn
    /// again only once it's shown larger than that. Never more than the decoder's limit.
    fn target_side(&self, rect: Rect) -> Target {
        let max = image_decode::MAX_SIDE;
        let needed = rect.size.width.max(rect.size.height).ceil().max(1.0) as u32;
        if self.svg {
            let ask =
                (needed.saturating_add(needed / 4)).div_ceil(SVG_STEP).saturating_mul(SVG_STEP);
            return Target { need: needed.min(max), ask: ask.clamp(SVG_STEP, max) };
        }
        let full = self.original.0.max(self.original.1).clamp(1, max);
        let mut side = full;
        while side / 2 >= needed && side / 2 >= MIN_SIDE {
            side /= 2;
        }
        Target { need: side, ask: side }
    }

    /// Takes a finished decode, starts the one needed for `rect`, and returns what to draw:
    /// the image, or the small copy while it's decoded (`None`: nothing yet).
    fn image_for(
        &mut self,
        rect: Rect,
        waker: Option<&Arc<dyn Fn() + Send + Sync>>,
    ) -> Option<ImageData> {
        let arrived = self.decoding.as_ref().and_then(|d| match d.result.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            // Its job gave nothing back (it panicked): not asked for again.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err(std::io::Error::other("the decoder stopped")))
            }
        });
        if let Some(result) = arrived
            && let Some(decoding) = self.decoding.take()
        {
            let side = decoding.side;
            match result {
                Ok(decoded) => {
                    if self.thumb.is_none() {
                        let (w, h, small) = image_decode::shrink(
                            decoded.width,
                            decoded.height,
                            &decoded.pixels,
                            THUMB_SIDE,
                        );
                        self.thumb = Some(ImageData::new(w, h, small));
                    }
                    let (w, h, pixels) =
                        self.transform.apply_owned(decoded.width, decoded.height, decoded.pixels);
                    let image = ImageData::gpu_only(w, h, pixels);
                    self.shown = Some(Shown { image, side, transform: self.transform });
                }
                Err(e) => {
                    self.failed = true;
                    self.message = Some(i18n::tr!("image.unreadable", error = e.to_string()));
                }
            }
            // The decoder's full-size buffers are gone: their pages go back to the system.
            kernel::memory::release_free();
        }
        let target = self.target_side(rect);
        let usable = self
            .shown
            .as_ref()
            .filter(|s| s.transform == self.transform && !s.image.is_lost())
            .map(|s| (s.image.clone(), s.side));
        let sharp = usable.as_ref().is_some_and(|(_, side)| *side >= target.need);
        if !sharp && !self.failed && self.decoding.as_ref().is_none_or(|d| d.side < target.need) {
            self.decode(target.ask, waker.cloned());
        }
        if let Some((image, _)) = usable {
            return Some(image);
        }
        // The small copy, with the transform (built once per transform).
        if self.thumb_shown.as_ref().is_none_or(|(t, _)| *t != self.transform) {
            let thumb = self.thumb.as_ref()?;
            let (w, h, pixels) =
                thumb.with_pixels(|p| self.transform.apply(thumb.width, thumb.height, p))?;
            self.thumb_shown = Some((self.transform, ImageData::new(w, h, pixels)));
        }
        self.thumb_shown.as_ref().map(|(_, image)| image.clone())
    }

    /// Decodes the image with a long side of `side`, on a background thread; one asked
    /// before it and still queued is let go.
    fn decode(&mut self, side: u32, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.path.clone();
        let svg = self.svg;
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let latest = Arc::clone(&self.generation);
        DECODES.spawn(kernel::Priority::Interactive, None, move || {
            let Some(result) = decode_job(&latest, generation, &path, side, svg) else { return };
            let _ = tx.send(result);
            if let Some(wake) = waker {
                wake();
            }
        });
        self.decoding = Some(Decoding { side, result: rx });
    }

    /// `transform` written to the file (`bytes` long now): the file is the new base, decoded
    /// again from it (a decode of the old one still on its way let go).
    fn saved(&mut self, transform: Transform, bytes: u64) {
        self.original = transform.size(self.original.0, self.original.1);
        self.shown = None;
        self.thumb = None;
        self.thumb_shown = None;
        self.decoding = None;
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.failed = false;
        self.transform = Transform::default();
        self.bytes = bytes;
        self.message = Some(i18n::t("image.saved"));
    }

    /// A save that failed, said (an animation or an icon of several sizes: in the user's
    /// language); the turn stays, unsaved.
    fn save_failed(&mut self, e: &std::io::Error) {
        self.message = Some(if e.kind() == std::io::ErrorKind::Unsupported {
            i18n::t("image.save_several")
        } else {
            i18n::tr!("image.save_failed", error = e.to_string())
        });
    }

    fn run(&mut self, tool: Tool) -> bool {
        match tool {
            Tool::ZoomIn => self.zoom(1.25, self.center()),
            Tool::ZoomOut => self.zoom(0.8, self.center()),
            Tool::Fit => {
                self.zoom = None;
                self.pan = Point::new(0.0, 0.0);
                true
            }
            Tool::Actual => {
                // 1:1 in logical pixels (like the real size on screen).
                let factor = self.scale / self.current_zoom();
                self.zoom(factor, self.center())
            }
            // While a save is written, the image stays as it's being saved.
            Tool::RotateLeft | Tool::RotateRight | Tool::FlipHorizontal | Tool::FlipVertical
                if self.saving.is_some() =>
            {
                false
            }
            Tool::RotateLeft => {
                self.set_transform(self.transform.rotate(false));
                true
            }
            Tool::RotateRight => {
                self.set_transform(self.transform.rotate(true));
                true
            }
            Tool::FlipHorizontal => {
                self.set_transform(self.transform.flip_horizontal());
                true
            }
            Tool::FlipVertical => {
                self.set_transform(self.transform.flip_vertical());
                true
            }
            Tool::Background => {
                self.background = match self.background {
                    Background::Checker => Background::Dark,
                    Background::Dark => Background::Light,
                    Background::Light => Background::Checker,
                };
                true
            }
            Tool::Save => {
                if let Err(e) = self.save() {
                    self.save_failed(&e);
                }
                true
            }
        }
    }
}

/// A decode on the pool: `None` when a later one was asked for meanwhile (or the view let
/// its images go), so a superseded decode never reads the file.
fn decode_job(
    latest: &AtomicU64,
    generation: u64,
    path: &Path,
    side: u32,
    svg: bool,
) -> Option<std::io::Result<image_decode::Decoded>> {
    if latest.load(Ordering::Relaxed) != generation {
        return None;
    }
    // The file read: counted on the module's row (its disk).
    DISK.read(std::fs::metadata(path).map_or(0, |m| m.len()));
    Some(if svg {
        image_decode::load_svg_at_side(path, side)
    } else {
        image_decode::load_fitting(path, side)
    })
}

impl Viewer for ImageViewer {
    /// Unseen for a while (or the caches cleared): the decoded image and its small copies
    /// go (RAM and GPU); decoded again from the file when shown.
    fn purge(&mut self) {
        self.shown = None;
        self.thumb = None;
        self.thumb_shown = None;
        self.decoding = None;
        // What's still queued is let go.
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.failed = false;
    }

    /// The image shown and its small copies: in memory until uploaded, then on the GPU.
    fn memory(&self) -> ui_components::ViewMemory {
        let images = self
            .shown
            .iter()
            .map(|s| &s.image)
            .chain(self.thumb.iter())
            .chain(self.thumb_shown.iter().map(|(_, i)| i));
        let mut memory = ui_components::ViewMemory::default();
        for image in images {
            memory.ram += image.memory() as u64;
            memory.gpu += image.gpu_memory();
        }
        memory
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.bounds = ctx.bounds;
        self.scale = ctx.scale;
        self.waker.clone_from(&ctx.waker);
        self.clamp_pan();
        let s = ctx.scale;
        let px = |v: f32| (v * s).round();
        let palette = ctx.palette;
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        layer.quad(Quad::filled(ctx.bounds, palette.editor_background));
        let rect = self.rect();
        // Background under the image (transparency shows); only in the visible area.
        if let Some(visible) = rect.intersect(&ctx.bounds) {
            match self.background {
                Background::Dark => {
                    layer.quad(Quad::filled(visible, DARK_GROUND));
                }
                Background::Light => {
                    layer.quad(Quad::filled(visible, Rgba::WHITE));
                }
                Background::Checker => {
                    let cell = px(8.0).max(4.0);
                    layer.quad(Quad::filled(visible, palette.text.with_alpha(0.04)));
                    let dark = palette.text.with_alpha(0.09);
                    let first_row = ((visible.min_y() - rect.min_y()) / cell).floor() as i32;
                    let last_row = ((visible.max_y() - rect.min_y()) / cell).ceil() as i32;
                    let first_col = ((visible.min_x() - rect.min_x()) / cell).floor() as i32;
                    let last_col = ((visible.max_x() - rect.min_x()) / cell).ceil() as i32;
                    for row in first_row.max(0)..last_row {
                        let y = rect.min_y() + row as f32 * cell;
                        let start = first_col.max(0);
                        let start = start + (start + row).rem_euclid(2);
                        for col in (start..last_col).step_by(2) {
                            let x = rect.min_x() + col as f32 * cell;
                            if let Some(c) = Rect::new(x, y, cell, cell).intersect(&visible) {
                                layer.quad(Quad::filled(c, dark));
                            }
                        }
                    }
                }
            }
        }
        if let Some(image) = self.image_for(rect, ctx.waker.as_ref()) {
            layer.image(&image, rect);
        }
        layer.quad(
            Quad::filled(rect, Rgba::TRANSPARENT).bordered(s.round().max(1.0), palette.border),
        );

        // Bottom badge: dimensions, format, file size (or a message).
        let (w, h) = self.size();
        let dirty = if self.is_dirty() {
            format!("  ·  {}", i18n::t("image.modified"))
        } else {
            String::new()
        };
        let info = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} × {}  ·  {}  ·  {}{dirty}",
                w as u32,
                h as u32,
                self.format,
                i18n::bytes(self.bytes)
            )
        });
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.0));
        let metrics = ctx.text.metrics(&font);
        let shaped = ctx.text.shape_line(&info, &font);
        let (pw, ph) = (shaped.width + px(24.0), px(28.0));
        let b = ctx.bounds;
        let pill = Rect::new(
            (b.min_x() + (b.size.width - pw) / 2.0).round(),
            b.max_y() - ph - px(14.0),
            pw.round(),
            ph,
        );
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        layer.quad(
            Quad::filled(pill, palette.popup_background.with_alpha(0.92))
                .rounded(ph / 2.0)
                .bordered(s.round().max(1.0), palette.popup_border),
        );
        draw_text(
            layer,
            &shaped,
            pill.min_x() + px(12.0),
            centered_baseline(pill.min_y(), ph, &metrics),
            palette.text_muted,
        );

        let zoom = (self.current_zoom() / s * 100.0).round() as u32;
        let can_save = self.is_dirty() && image_decode::can_save(&self.path);
        let painter = Painter { text: ctx.text, scene: ctx.scene, palette, icons: ctx.icons };
        self.toolbar.paint(ctx.bounds, zoom, can_save, s, painter);
    }

    fn poll(&mut self) -> bool {
        // A save written: the file is the image's new base (or it wasn't: said).
        let done = self.saving.as_ref().and_then(|s| match s.result.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err(std::io::Error::other("the save stopped")))
            }
        });
        if let Some(result) = done
            && let Some(saving) = self.saving.take()
        {
            match result {
                Ok(bytes) => self.saved(saving.transform, bytes),
                Err(e) => self.save_failed(&e),
            }
            return true;
        }
        // A decode in progress woke the app (or will): drawing takes its result.
        self.decoding.is_some()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let before = self.pan;
        self.pan = Point::new(self.pan.x - dx, self.pan.y - dy);
        self.clamp_pan();
        self.pan != before
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.current_zoom() / self.scale.max(0.01))
    }

    fn reset_zoom(&mut self) -> bool {
        // 100% = one image pixel per logical pixel.
        self.zoom = Some(self.scale);
        self.pan = Point::new(0.0, 0.0);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        let old = self.current_zoom();
        let new = (old * factor).clamp(0.02 * self.scale, 32.0 * self.scale);
        if (new - old).abs() < f32::EPSILON {
            return false;
        }
        // The point under the mouse stays in place.
        let center = Point::new(
            self.bounds.min_x() + self.bounds.size.width / 2.0 + self.pan.x,
            self.bounds.min_y() + self.bounds.size.height / 2.0 + self.pan.y,
        );
        let k = new / old;
        self.pan = Point::new(
            self.pan.x + (center.x - at.x) * (k - 1.0),
            self.pan.y + (center.y - at.y) * (k - 1.0),
        );
        self.zoom = Some(new);
        self.clamp_pan();
        true
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(tool) = self.toolbar.hit(p) {
            self.run(tool);
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        self.toolbar.hover(p)
    }

    fn hover_tells_changes(&self) -> bool {
        true
    }

    /// A hand over a button that can be pressed.
    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.toolbar.hit(p).map(|_| ui_components::ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.toolbar.tooltip_at(p)
    }

    /// Its actions ([`ACTIONS`]).
    fn command(&mut self, id: &str) -> bool {
        let tool = match id {
            "zoom_in" => Tool::ZoomIn,
            "zoom_out" => Tool::ZoomOut,
            "fit" => Tool::Fit,
            "actual" => Tool::Actual,
            "rotate_right" => Tool::RotateRight,
            "rotate_left" => Tool::RotateLeft,
            "flip_horizontal" => Tool::FlipHorizontal,
            "flip_vertical" => Tool::FlipVertical,
            "background" => Tool::Background,
            _ => return false,
        };
        self.run(tool)
    }

    /// Turned and not saved (a save being written counts as saved; failing, it's not).
    fn is_dirty(&self) -> bool {
        !self.transform.is_identity() && self.saving.is_none()
    }

    /// Written on a worker (the whole image read, turned, encoded and written), told by a
    /// poll; without a waker (tests: nothing would poll), here and now.
    fn save(&mut self) -> std::io::Result<()> {
        if !self.is_dirty() {
            return Ok(());
        }
        let transform = self.transform;
        let Some(waker) = self.waker.clone() else {
            image_decode::save_transformed(&self.path, transform)?;
            let bytes = std::fs::metadata(&self.path).map_or(self.bytes, |m| m.len());
            self.saved(transform, bytes);
            return Ok(());
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.path.clone();
        DECODES.spawn(kernel::Priority::Interactive, None, move || {
            let result = image_decode::save_transformed(&path, transform)
                .map(|()| std::fs::metadata(&path).map_or(0, |m| m.len()));
            let _ = tx.send(result);
            waker();
        });
        self.saving = Some(Saving { transform, result: rx });
        Ok(())
    }

    fn status(&self) -> Option<String> {
        let (w, h) = self.size();
        let zoom = (self.current_zoom() / self.scale * 100.0).round();
        Some(format!("{} × {} · {} · {zoom}%", w as u32, h as u32, self.format))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
