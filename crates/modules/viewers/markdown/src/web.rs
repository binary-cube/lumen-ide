//! Images from the web (`http(s)://`): downloaded on a pool of their own (never while
//! painting), kept in the app's cache folder (`web-images`, at most 200 MB, the least used
//! going first, emptied by "Clear and Rebuild Caches"), then shown as the files they are.
//! Limits: 20 MB, 20 s, 5 redirects, only `image/…` answers; no cookies, no credentials,
//! a plain User-Agent; nothing asked when the setting is off. A downloaded SVG is drawn
//! without anything it would load itself.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};

use web_fetch::{DiskCache, Limits};

/// The threads downloading.
pub const WEB_THREAD: &str = "markdown-web";

/// The cache folder's cap.
const CAP: u64 = 200 * 1024 * 1024;

/// Where a web image is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Pending,
    Ready(PathBuf),
    /// Why not (shown).
    Failed(String),
}

/// The cache folder, if the app has one.
pub fn cache() -> Option<DiskCache> {
    files::places::cache_dir().map(|d| DiskCache::new(d.join("web-images"), CAP))
}

/// Whether `path` was downloaded (drawn as untrusted: an SVG loads nothing of its own).
pub fn downloaded(path: &std::path::Path) -> bool {
    cache().is_some_and(|c| path.starts_with(c.dir()))
}

/// The extension a content type is kept with (the decoder knows SVGs by theirs).
fn extension(kind: &str) -> &'static str {
    match kind {
        "image/svg+xml" => "svg",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => "png",
    }
}

/// The downloads of a preview: done, failed, or under way.
#[derive(Default)]
pub struct WebImages {
    known: HashMap<String, Fetch>,
    pending: HashMap<String, Receiver<Fetch>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// What every preview downloads with (one at a time per URL across previews).
fn pool() -> kernel::Pool {
    static POOL: OnceLock<kernel::Pool> = OnceLock::new();
    POOL.get_or_init(|| kernel::Pool::new(WEB_THREAD, 2, kernel::work::IDLE)).clone()
}

/// The URLs being downloaded by any preview (another asking waits for the same file).
fn in_flight() -> &'static Mutex<std::collections::HashSet<String>> {
    static FLIGHT: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    FLIGHT.get_or_init(Default::default)
}

impl WebImages {
    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    pub fn loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Takes the downloads that ended: `true` if one did.
    pub fn poll(&mut self) -> bool {
        let mut done = Vec::new();
        self.pending.retain(|url, rx| match rx.try_recv() {
            Ok(f) => {
                done.push((url.clone(), f));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        let any = !done.is_empty();
        self.known.extend(done);
        any
    }

    pub fn clear(&mut self) {
        self.known.clear();
        self.pending.clear();
    }

    /// Where `url`'s image is: on disk (from the cache, or downloaded now), being
    /// downloaded, or why it can't be.
    pub fn get(&mut self, url: &str) -> Fetch {
        if let Some(f) = self.known.get(url) {
            if let Fetch::Ready(path) = f
                && !path.exists()
            {
                // Its file was cleared: asked again.
                self.known.remove(url);
            } else {
                return f.clone();
            }
        }
        if self.pending.contains_key(url) {
            return Fetch::Pending;
        }
        if !crate::settings::web_images() {
            return Fetch::Failed(i18n::t("markdown.image_web_off"));
        }
        if !web_fetch::allowed(url) {
            return Fetch::Failed(i18n::t("markdown.image_refused"));
        }
        let Some(cache) = cache() else { return Fetch::Failed(i18n::t("markdown.image_refused")) };
        if let Some(path) = cache.get(url) {
            let f = Fetch::Ready(path);
            self.known.insert(url.to_owned(), f.clone());
            return f;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (url_owned, wake) = (url.to_owned(), self.wake.clone());
        let flight = in_flight();
        pool().spawn(kernel::Priority::Background, None, move || {
            // Another preview fetching the same file: wait for its copy.
            loop {
                let mut busy = flight.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if busy.insert(url_owned.clone()) {
                    break;
                }
                drop(busy);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let result = match cache.get(&url_owned) {
                Some(path) => Fetch::Ready(path),
                None => match web_fetch::fetch(&url_owned, Limits::default(), &|k| {
                    k.starts_with("image/")
                }) {
                    Ok(f) => match cache.put(&url_owned, extension(&f.kind), &f.bytes) {
                        Ok(path) => Fetch::Ready(path),
                        Err(e) => Fetch::Failed(e.to_string()),
                    },
                    Err(e) => Fetch::Failed(e.to_string()),
                },
            };
            flight.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&url_owned);
            let _ = tx.send(result);
            if let Some(wake) = wake {
                wake();
            }
        });
        self.pending.insert(url.to_owned(), rx);
        Fetch::Pending
    }
}

#[cfg(test)]
#[path = "../tests/unit/web.rs"]
mod tests;
