//! Local images for the layout and the paint: their sizes read from their headers and
//! their stamps looked at again on a worker, their pixels decoded at the size shown on a
//! pool of their own; neither the layout nor the paint reads the disk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use render_api::ImageData;

use super::{Item, Layout};

/// What's known of an image file: being looked at, unreadable, or its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    Pending,
    Missing,
    /// A web image that couldn't be had (why).
    Failed(String),
    Size(u32, u32),
}

/// A file's modification time, as a worker last read it (an image edited on disk is read
/// again).
pub(crate) type Stamp = Option<SystemTime>;

/// A file decoded for a width (rounded up to 256 pixels), at the stamp it was read at.
type DecodeKey = (PathBuf, Stamp, u32);

/// What a worker learned of an image file: its stamp and its size.
type Probed = (Stamp, Probe);

/// The files whose stamp changed since they were probed, probed again.
type Rechecked = Vec<(PathBuf, Probed)>;

/// How often the files of a document being painted are looked at again (on a worker).
const RECHECK: Duration = Duration::from_secs(2);

/// Local images' sizes: the layout asks only these (read from their headers, in the
/// background), each file's modification time read with it on the same worker. Neither
/// the layout nor the paint reads the disk: while the document is painted a worker looks
/// at the stamps again every [`RECHECK`] (an edited image is probed again, and its
/// picture, keyed by the new stamp, decoded again).
pub struct ImageSizes {
    /// Each file's stamp and size, by path.
    pub(super) sizes: HashMap<PathBuf, Probed>,
    probing: HashMap<PathBuf, Receiver<Probed>>,
    /// The files the layout being built uses (the others go when it's done).
    used: HashSet<PathBuf>,
    /// The stamps being looked at again, and when they last were.
    rechecking: Option<Receiver<Rechecked>>,
    checked: Option<Instant>,
    pub(super) recheck_every: Duration,
    /// The images from the web (downloaded into the cache folder).
    web: crate::web::WebImages,
    /// Wakes the app when something arrives.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Worked out right away (tests).
    inline: bool,
}

impl Default for ImageSizes {
    fn default() -> Self {
        Self {
            sizes: HashMap::new(),
            probing: HashMap::new(),
            used: HashSet::new(),
            rechecking: None,
            checked: None,
            recheck_every: RECHECK,
            web: crate::web::WebImages::default(),
            wake: None,
            inline: false,
        }
    }
}

/// Local images' pixels: decoded when an image is about to be shown (a screen away), at
/// the size it's shown — never bigger — on the `image-decode` pool, never while painting,
/// under the stamp the layout gave its picture. They live only on the GPU once drawn
/// (`ImageData::gpu_only`); one whose texture was freed (out of sight: the renderer, the
/// previews' budget) is decoded again when it's shown again.
#[derive(Default)]
pub struct ImageCache {
    pub(super) decoded: HashMap<DecodeKey, Option<ImageData>>,
    decoding: HashMap<DecodeKey, Receiver<Option<ImageData>>>,
    /// Wakes the app when something arrives.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Worked out right away (tests).
    inline: bool,
}

/// A file's modification time (on a worker).
fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// An image's stamp and its size, from its header (the stamp read first: an edit made
/// meanwhile is seen at the next look).
fn probe(path: &Path) -> Probed {
    let modified = stamp(path);
    let size = match image_decode::probe(path) {
        Ok(((w, h), _)) if w > 0 && h > 0 => Probe::Size(w, h),
        _ => Probe::Missing,
    };
    (modified, size)
}

/// The files of `known` whose stamp is no longer the one known, probed again.
fn recheck(known: Vec<(PathBuf, Stamp)>) -> Rechecked {
    known
        .into_iter()
        .filter(|(path, modified)| stamp(path) != *modified)
        .map(|(path, _)| {
            let probed = probe(&path);
            (path, probed)
        })
        .collect()
}

/// An image read and decoded at most `width` pixels wide (an SVG at its size × `scale`):
/// the pixels on the GPU once drawn.
fn decode(path: &Path, width: u32, scale: f32) -> Option<ImageData> {
    crate::DISK.read(std::fs::metadata(path).map_or(0, |m| m.len()));
    let decoded = if crate::web::downloaded(path) {
        image_decode::load_untrusted_for_width(path, width, scale)
    } else {
        image_decode::load_for_width(path, width, scale)
    };
    decoded.ok().map(|d| ImageData::gpu_only(d.width, d.height, d.pixels))
}

#[cfg(test)]
impl ImageSizes {
    /// Working out while laying out (tests of what's cached, without waiting).
    pub(crate) fn inline() -> Self {
        Self { inline: true, ..Self::default() }
    }
}

#[cfg(test)]
impl ImageCache {
    /// Decoding while painting (tests of what's cached, without waiting).
    pub(crate) fn inline() -> Self {
        Self { inline: true, ..Self::default() }
    }
}

/// The threads reading and decoding a document's images.
pub const DECODE_THREAD: &str = "markdown-image";

/// The previews' decodes, shared by all (each its own pool: as many full decodes at once as
/// previews open).
static DECODES: std::sync::LazyLock<kernel::Pool> =
    std::sync::LazyLock::new(|| kernel::Pool::new(DECODE_THREAD, 2, kernel::work::IDLE));

/// The width an image shown `width` pixels wide is decoded at.
pub(crate) fn decode_width(width: f32) -> u32 {
    ((width.max(1.0) as u32).div_ceil(256) * 256).min(image_decode::MAX_SIDE)
}

/// What came out of a worker's channel: taken, still to come, or gone.
fn take<T>(rx: &Receiver<T>) -> Option<Option<T>> {
    match rx.try_recv() {
        Ok(v) => Some(Some(v)),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
    }
}

impl ImageSizes {
    pub fn clear(&mut self) {
        self.sizes.clear();
        self.probing.clear();
        self.used.clear();
        self.rechecking = None;
        self.checked = None;
        self.web.clear();
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.web.set_waker(wake.clone());
        self.wake = wake;
    }

    /// Sizes are being read (or downloaded). A look at the stamps isn't waited for: it
    /// wakes the app if something changed.
    pub fn loading(&self) -> bool {
        !self.probing.is_empty() || self.web.loading()
    }

    /// Takes what arrived: `true` if a size did, or an image changed on disk (the layout is
    /// built again). Every [`RECHECK`] the stamps are looked at again.
    pub fn poll(&mut self) -> bool {
        let mut sizes = Vec::new();
        self.probing.retain(|path, rx| match take(rx) {
            None => true,
            Some(p) => {
                sizes.extend(p.map(|p| (path.clone(), p)));
                false
            }
        });
        let any = !sizes.is_empty();
        self.sizes.extend(sizes);
        self.start_recheck();
        let changed = self.take_recheck();
        any | self.web.poll() | changed
    }

    /// The stamps looked at again arrived: `true` if a file changed (its new size kept).
    fn take_recheck(&mut self) -> bool {
        let Some(rx) = &self.rechecking else { return false };
        let Some(changed) = take(rx) else { return false };
        self.rechecking = None;
        let mut any = false;
        // Only files still known: one the document dropped meanwhile isn't brought back.
        for (path, probed) in changed.unwrap_or_default() {
            if let Some(known) = self.sizes.get_mut(&path) {
                *known = probed;
                any = true;
            }
        }
        any
    }

    /// A look at the known files' stamps, on a worker, once [`RECHECK`] passed since the
    /// last (only while the document is painted: nothing while it's out of sight).
    fn start_recheck(&mut self) {
        if self.rechecking.is_some() || self.sizes.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.checked.is_some_and(|t| now.duration_since(t) < self.recheck_every) {
            return;
        }
        if self.checked.replace(now).is_none() {
            // Just probed: the first look waits a whole interval.
            return;
        }
        // Every file known, a missing one too (it may appear).
        let known: Vec<(PathBuf, Stamp)> =
            self.sizes.iter().map(|(path, (modified, _))| (path.clone(), *modified)).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        if self.inline {
            let _ = tx.send(recheck(known));
        } else {
            let wake = self.wake.clone();
            DECODES.spawn(kernel::Priority::Background, None, move || {
                let changed = recheck(known);
                let any = !changed.is_empty();
                let _ = tx.send(changed);
                if any && let Some(wake) = wake {
                    wake();
                }
            });
        }
        self.rechecking = Some(rx);
    }

    /// A layout starts: what it uses is recorded.
    pub(crate) fn begin(&mut self) {
        self.used.clear();
    }

    /// The layout is done: what it didn't use goes.
    pub(crate) fn end(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.sizes.retain(|path, _| used.contains(path));
        self.used = used;
    }

    /// Image `src`'s size and the stamp it came with (asked in the background the first
    /// time; the disk is never read here).
    pub(crate) fn size(&mut self, dir: &Path, src: &str) -> (Probe, PathBuf, Stamp) {
        let lower = src.get(..8).unwrap_or(src).to_ascii_lowercase();
        let path = if lower.starts_with("https://") || lower.starts_with("http://") {
            match self.web.get(src) {
                crate::web::Fetch::Ready(path) => path,
                crate::web::Fetch::Pending => return (Probe::Pending, PathBuf::new(), None),
                crate::web::Fetch::Failed(why) => {
                    return (Probe::Failed(why), PathBuf::new(), None);
                }
            }
        } else {
            dir.join(src)
        };
        if src.starts_with("data:") || (src.contains("://") && !crate::web::downloaded(&path)) {
            return (Probe::Missing, path, None);
        }
        self.used.insert(path.clone());
        if let Some((modified, p)) = self.sizes.get(&path) {
            return (p.clone(), path, *modified);
        }
        if self.inline {
            let (modified, p) = probe(&path);
            self.sizes.insert(path.clone(), (modified, p.clone()));
            return (p, path, modified);
        }
        if !self.probing.contains_key(&path) {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            let p = path.clone();
            DECODES.spawn(kernel::Priority::Interactive, None, move || {
                let _ = tx.send(probe(&p));
                if let Some(wake) = wake {
                    wake();
                }
            });
            self.probing.insert(path.clone(), rx);
        }
        (Probe::Pending, path, None)
    }
}

impl ImageCache {
    /// What its images hold: (in memory, on the GPU), bytes.
    pub fn memory(&self) -> (u64, u64) {
        self.decoded.values().flatten().fold((0, 0), |(ram, gpu), image| {
            (ram + image.memory() as u64, gpu + image.gpu_memory())
        })
    }

    pub fn clear(&mut self) {
        self.decoded.clear();
        self.decoding.clear();
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    /// Pixels are being decoded.
    pub fn loading(&self) -> bool {
        !self.decoding.is_empty()
    }

    /// Takes the pixels that arrived (they only redraw).
    pub fn poll(&mut self) {
        let mut images = Vec::new();
        self.decoding.retain(|key, rx| match take(rx) {
            None => true,
            Some(i) => {
                images.extend(i.map(|i| (key.clone(), i)));
                false
            }
        });
        self.decoded.extend(images);
    }

    /// A layout was put in place: the pixels of files (or stamps) it doesn't show go.
    pub(crate) fn keep(&mut self, layout: &Layout) {
        let shown: HashSet<(&Path, Stamp)> = layout
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Picture { path, stamp, .. } => Some((path.as_path(), *stamp)),
                _ => None,
            })
            .collect();
        self.decoded.retain(|(path, modified, _), _| shown.contains(&(path.as_path(), *modified)));
    }

    /// The pixels of `path` (as of `stamp`) decoded `width` pixels wide (an SVG at most its
    /// size × `scale`), if they're there; otherwise asked for. The disk is never read here.
    pub(crate) fn image(
        &mut self,
        path: &Path,
        stamp: Stamp,
        width: u32,
        scale: f32,
    ) -> Option<ImageData> {
        let key = (path.to_path_buf(), stamp, width);
        match self.decoded.get(&key) {
            Some(Some(image)) if !image.is_lost() => return Some(image.clone()),
            Some(None) => return None,
            Some(Some(_)) => {
                // Freed out of sight: decoded again.
                self.decoded.remove(&key);
            }
            None => {}
        }
        if self.inline {
            let image = decode(path, width, scale);
            self.decoded.insert(key, image.clone());
            return image;
        }
        if !self.decoding.contains_key(&key) {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            let p = path.to_path_buf();
            DECODES.spawn(kernel::Priority::Interactive, None, move || {
                let _ = tx.send(decode(&p, width, scale));
                if let Some(wake) = wake {
                    wake();
                }
            });
            self.decoding.insert(key, rx);
        }
        None
    }
}
